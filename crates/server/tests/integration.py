#!/usr/bin/env python3
"""Real HTTP/PostgreSQL/worker regression. Requires built binaries and an existing dev PostgreSQL container.
Run: cargo build --workspace && python3 crates/server/tests/integration.py
Uses a unique disposable database/media directory; never clears the development database.
"""
import base64
import concurrent.futures
import datetime as dt
import http.cookiejar
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid
from zoneinfo import ZoneInfo

ROOT = Path(__file__).resolve().parents[3]
CONTAINER = os.environ.get("BEWEFT_TEST_POSTGRES_CONTAINER", "beweft-dev-postgres")
DB = "beweft_test_" + uuid.uuid4().hex[:12]
DB_PORT = os.environ.get("BEWEFT_TEST_DB_PORT", "55439")
DB_USER = os.environ.get("BEWEFT_TEST_DB_USER", "beweft")
DB_PASSWORD = os.environ.get("BEWEFT_TEST_DB_PASSWORD", "beweft-local-development")
processes = []


def sql(command):
    return subprocess.run(["docker", "exec", CONTAINER, "psql", "-U", DB_USER, "-d", DB, "-v", "ON_ERROR_STOP=1", "-c", command], check=True, capture_output=True, text=True)


def run():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    origin = f"http://127.0.0.1:{port}"
    with tempfile.TemporaryDirectory(prefix="beweft-integration-") as temporary:
        env = dict(os.environ, DATABASE_URL=f"postgres://{DB_USER}:{DB_PASSWORD}@localhost:{DB_PORT}/{DB}", BEWEFT_PUBLIC_URL=origin, BEWEFT_BIND=f"127.0.0.1:{port}", BEWEFT_MEDIA_DIR=temporary, BEWEFT_SETUP_SECRET="isolated-integration-setup-secret", BEWEFT_ENABLE_FIXTURES="true")
        for name in ["beweft-server", "beweft-worker"]:
            processes.append(subprocess.Popen([str(ROOT / "target/debug" / name)], cwd=ROOT, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

        def call(path, body=None, method=None, expected=200, token=None, headers=None):
            head = {"X-Beweft-Request": "1", "Origin": origin}
            head.update(headers or {})
            if body is not None:
                head["Content-Type"] = "application/json"
            if token:
                head["Authorization"] = "Bearer " + token
            request = urllib.request.Request(origin + "/api/v1" + path, data=None if body is None else json.dumps(body).encode(), headers=head, method=method or ("POST" if body is not None else "GET"))
            try:
                with client.open(request, timeout=10) as response:
                    status, raw = response.status, response.read()
            except urllib.error.HTTPError as error:
                status, raw = error.code, error.read()
            assert status == expected, (path, status, expected, raw.decode()[:500])
            result = json.loads(raw)
            return result.get("data") if status == 200 else result

        def wait_state(publication, desired):
            for _ in range(40):
                result = next(p for p in call("/publications") if p["id"] == publication["id"])
                if result["state"] == desired:
                    return result
                time.sleep(0.2)
            raise AssertionError((desired, result))

        for _ in range(100):
            try:
                call("/auth/status")
                break
            except (OSError, urllib.error.URLError):
                assert all(p.poll() is None for p in processes), "Backend process exited during startup"
                time.sleep(0.1)
        else:
            raise AssertionError("Server did not start")
        call("/auth/bootstrap", {"setup_secret": env["BEWEFT_SETUP_SECRET"], "email": "test@example.com", "password": "Isolated-integration-password!"})
        call("/auth/bootstrap", {"setup_secret": env["BEWEFT_SETUP_SECRET"], "email": "second@example.com", "password": "Isolated-integration-password!"}, expected=409)
        call("/brands", {"name": "Rejected"}, headers={"Origin": "https://evil.example"}, expected=403)
        call("/brands", {"name": "Rejected"}, headers={"X-Beweft-Request": ""}, expected=400)
        call("/posts/not-a-uuid", expected=400)
        brand = call("/brands", {"name": "Integration brand"})
        other = call("/brands", {"name": "Other brand"})
        channel = call("/channels", {"brand_id": brand["id"], "name": "Manual", "provider": "manual", "timezone": "America/Chicago"})
        restricted = call("/tokens", {"name": "Read one brand", "scopes": ["read"], "brand_ids": [brand["id"]]})
        assert [b["id"] for b in call("/brands", token=restricted["token"])] == [brand["id"]]
        call("/posts", {"brand_id": brand["id"], "text": "Forbidden"}, token=restricted["token"], expected=403)
        call("/channels?brand_id=" + other["id"], token=restricted["token"], expected=403)
        call("/tokens", token=restricted["token"], expected=403)
        post = call("/posts", {"brand_id": brand["id"], "text": "Original", "variants": [{"channel_id": channel["id"], "text": "Variant snapshot"}], "options": {"privacy": "private"}})
        later = (dt.datetime.now(dt.timezone.utc) + dt.timedelta(hours=1)).isoformat()
        publication = call(f"/posts/{post['id']}/schedule", {"channel_ids": [channel["id"]], "scheduled_at": later})[0]
        call(f"/posts/{post['id']}", {"version": 1, "text": "Changed", "variants": []}, "PATCH")
        call(f"/posts/{post['id']}", {"version": 1, "text": "Stale"}, "PATCH", expected=409)
        snapshot = next(p for p in call("/publications") if p["id"] == publication["id"])
        assert snapshot["payload"]["text"] == "Variant snapshot" and snapshot["payload"]["options"] == {"privacy": "private"}
        call(f"/publications/{publication['id']}/cancel", {})
        # Two simultaneous requests must reserve one exact channel slot only.
        racing = [call("/posts", {"brand_id": brand["id"], "text": f"Race {i}"}) for i in range(2)]
        def race(p):
            try:
                call(f"/posts/{p['id']}/schedule", {"channel_ids": [channel["id"]], "scheduled_at": later})
                return 200
            except AssertionError as error:
                assert error.args[0][1] == 409, error
                return 409
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            assert sorted(pool.map(race, racing)) == [200, 409]
        due = call(f"/posts/{post['id']}/schedule", {"channel_ids": [channel["id"]], "scheduled_at": dt.datetime.now(dt.timezone.utc).isoformat()})[0]
        wait_state(due, "manual_required")
        call(f"/publications/{due['id']}/manual-complete", {"note": "Isolated test; no network post"})
        attempts = call(f"/publications/{due['id']}/attempts")
        assert any(a.get("receipt", {}).get("verification") == "owner_attestation" for a in attempts)
        call(f"/publications/{due['id']}/retry", {}, expected=409)
        call(f"/posts/{post['id']}", method="DELETE", expected=409)
        call("/schedules", {"channel_id": channel["id"], "days": [1, 2, 3, 4, 5, 6, 7], "time": "09:00", "timezone": "America/Chicago"})
        scheduled = call("/posts", {"brand_id": brand["id"], "text": "Next local slot"})
        next_slot = call(f"/posts/{scheduled['id']}/schedule", {"channel_ids": [channel["id"]]})[0]
        assert dt.datetime.fromisoformat(next_slot["scheduled_at"].replace("Z", "+00:00")).astimezone(ZoneInfo("America/Chicago")).hour == 9
        call(f"/channels/{channel['id']}", {"paused": True}, "PATCH")
        paused = call("/posts", {"brand_id": brand["id"], "text": "Paused"})
        pending = call(f"/posts/{paused['id']}/schedule", {"channel_ids": [channel["id"]], "scheduled_at": dt.datetime.now(dt.timezone.utc).isoformat()})[0]
        time.sleep(2.5)
        assert next(p for p in call("/publications") if p["id"] == pending["id"])["state"] == "queued"
        sql(f"UPDATE publications SET state='sending',lease_until=now()-interval '1 minute' WHERE id='{pending['id']}'; INSERT INTO publication_attempts(id,publication_id,state,provider) VALUES(gen_random_uuid(),'{pending['id']}','sending','manual');")
        wait_state(pending, "uncertain")
        call(f"/publications/{pending['id']}/retry", {}, expected=409)
        call(f"/publications/{pending['id']}/reconcile", {"resolution": "not_published", "note": "No external send in this isolated fixture"})
        assert call(f"/publications/{pending['id']}/retry", {})["state"] == "queued"
        # Upload bytes, reference them, and prove deletion cannot break the draft.
        png = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=")
        boundary = "beweft-integration-boundary"
        multipart = f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="pixel.png"\r\nContent-Type: image/png\r\n\r\n'.encode() + png + f"\r\n--{boundary}--\r\n".encode()
        req = urllib.request.Request(origin + "/api/v1/media?brand_id=" + brand["id"], data=multipart, headers={"Content-Type": "multipart/form-data; boundary=" + boundary, "X-Beweft-Request": "1", "Origin": origin})
        with client.open(req) as response:
            media = json.load(response)["data"]
        with client.open(origin + media["url"]) as response:
            assert response.read() == png
        media_post = call("/posts", {"brand_id": brand["id"], "text": "Image", "media_ids": [media["id"]]})
        call(f"/media/{media['id']}", method="DELETE", expected=409)
        call(f"/posts/{media_post['id']}", method="DELETE")
        call(f"/media/{media['id']}", method="DELETE")
        assert not list(Path(temporary).iterdir()), "Deleted media file was retained"
        assert call("/jobs/health")["worker_healthy"]
        assert call("/analytics")["availability"] == "unavailable"
        assert call("/inbox")["availability"] == "unavailable"
        print("PASS: real HTTP, PostgreSQL and worker auth/CSRF/scoped access, immutable revisions, concurrent reservation, manual handoff, time zones, pause, crash quarantine/reconciliation, media lifecycle, honest unavailable surfaces")


if __name__ == "__main__":
    subprocess.run(["docker", "exec", CONTAINER, "createdb", "-U", DB_USER, DB], check=True)
    try:
        run()
    finally:
        for process in processes:
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
        subprocess.run(["docker", "exec", CONTAINER, "dropdb", "-U", DB_USER, "--force", DB], check=True)
