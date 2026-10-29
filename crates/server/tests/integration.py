#!/usr/bin/env python3
"""Real HTTP/PostgreSQL/worker regression. Requires built binaries and an existing dev PostgreSQL container.
Run: cargo build --workspace && python3 crates/server/tests/integration.py
Uses a unique disposable database/media directory; never clears the development database.
"""
import base64
import concurrent.futures
import datetime as dt
import http.cookiejar
import http.client
import re
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
import sys
sys.dont_write_bytecode = True
from workflows import exercise
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
        env = dict(os.environ, DATABASE_URL=f"postgres://{DB_USER}:{DB_PASSWORD}@localhost:{DB_PORT}/{DB}", BEWEFT_PUBLIC_URL=origin, BEWEFT_BIND=f"127.0.0.1:{port}", BEWEFT_MEDIA_DIR=temporary, BEWEFT_SETUP_SECRET="isolated-integration-setup-secret", BEWEFT_ENABLE_FIXTURES="true", BEWEFT_MAX_UPLOAD_BYTES=str(32*1024*1024), BEWEFT_MEDIA_QUOTA_BYTES=str(33*1024*1024), BEWEFT_FIXTURE_DELAY_MS="15000" if "--workflows-only" in sys.argv else "0")
        for name in ["beweft-server", "beweft-worker"]:
            processes.append(subprocess.Popen([str(ROOT / "target/debug" / name)], cwd=ROOT, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

        schema_document = None
        def validate(value, schema):
            if "$ref" in schema:
                return validate(value, schema_document["components"]["schemas"][schema["$ref"].split("/")[-1]])
            if "enum" in schema:
                assert value in schema["enum"], (value, schema)
            for key in ["oneOf", "anyOf"]:
                if key in schema:
                    for candidate in schema[key]:
                        try:
                            validate(value, candidate)
                            return
                        except AssertionError:
                            pass
                    raise AssertionError((value, schema))
            kind = schema.get("type")
            if isinstance(kind, list):
                for candidate in kind:
                    try:
                        validate(value, dict(schema, type=candidate))
                        return
                    except AssertionError:
                        pass
                raise AssertionError((value, schema))
            if kind == "null": assert value is None
            elif kind == "string": assert isinstance(value, str), value
            elif kind == "integer": assert isinstance(value, int) and not isinstance(value, bool), value
            elif kind == "number": assert isinstance(value, (int, float)) and not isinstance(value, bool), value
            elif kind == "boolean": assert isinstance(value, bool), value
            elif kind == "array":
                assert isinstance(value, list), value
                for item in value: validate(item, schema.get("items", {}))
            elif kind == "object":
                assert isinstance(value, dict), value
                assert all(key in value for key in schema.get("required", [])), (value, schema.get("required"))
                for key, field in schema.get("properties", {}).items():
                    if key in value: validate(value[key], field)

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
            if schema_document is not None:
                route = re.sub(r"/[0-9a-f]{8}-[0-9a-f-]{27,}", "/{id}", "/api/v1" + path.split("?")[0])
                operation = schema_document["paths"].get(route, {}).get((method or ("POST" if body is not None else "GET")).lower())
                if operation:
                    validate(result, operation["responses"][str(status)]["content"]["application/json"]["schema"])
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
        with client.open(origin + "/api-docs/openapi.json") as response:
            schema_document = json.load(response)
        assert schema_document["paths"]["/api/v1/posts/{id}/schedule"]["post"]["requestBody"]["required"]
        assert "version" in schema_document["components"]["schemas"]["SchedulePostRequest"]["required"]
        assert "video/mp4" in schema_document["paths"]["/api/v1/media/{id}/file"]["get"]["responses"]["200"]["content"]
        call("/auth/bootstrap", {"setup_secret": env["BEWEFT_SETUP_SECRET"], "email": "test@example.com", "password": "Isolated-integration-password!"})
        if "--workflows-only" in sys.argv:
            exercise(call, client, origin, sql)
            return
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
        admin_token = call("/tokens", {"name": "Admin bearer still not owner session", "scopes": ["admin"]})
        call("/tokens", token=admin_token["token"], expected=403)
        call("/tokens", {"name": "Forbidden nested token", "scopes": ["read"]}, token=admin_token["token"], expected=403)
        call(f"/tokens/{restricted['id']}", method="DELETE", token=admin_token["token"], expected=403)
        call("/auth/logout", {}, token=admin_token["token"], expected=403)
        assert call("/auth/me")["email"] == "test@example.com"
        for path, methods in [("/auth/logout", ["post"]), ("/tokens", ["get", "post"]), ("/tokens/{id}", ["delete"])]:
            for method in methods:
                assert schema_document["paths"]["/api/v1" + path][method]["security"] == [{"session": []}]

        post = call("/posts", {"brand_id": brand["id"], "text": "Original", "variants": [{"channel_id": channel["id"], "text": "Variant snapshot"}], "options": {"privacy": "private"}})
        later = (dt.datetime.now(dt.timezone.utc) + dt.timedelta(hours=1)).isoformat()
        publication = call(f"/posts/{post['id']}/schedule", {"version": post["version"], "channel_ids": [channel["id"]], "scheduled_at": later})[0]
        post = call(f"/posts/{post['id']}", {"version": 1, "text": "Changed", "variants": []}, "PATCH")
        call(f"/posts/{post['id']}/schedule", {"channel_ids": [channel["id"]], "scheduled_at": later}, expected=422)
        call(f"/posts/{post['id']}/schedule", {"version": 1, "channel_ids": [channel["id"]], "scheduled_at": (dt.datetime.now(dt.timezone.utc)+dt.timedelta(hours=2)).isoformat()}, expected=409)
        assert len(call("/publications")) == 1, "Stale scheduling must not create a publication"
        call(f"/posts/{post['id']}", {"version": 1, "text": "Stale"}, "PATCH", expected=409)
        snapshot = next(p for p in call("/publications") if p["id"] == publication["id"])
        assert snapshot["payload"]["text"] == "Variant snapshot" and snapshot["payload"]["options"] == {"privacy": "private"}
        call(f"/publications/{publication['id']}/cancel", {})
        # Two simultaneous requests must reserve one exact channel slot only.
        racing = [call("/posts", {"brand_id": brand["id"], "text": f"Race {i}"}) for i in range(2)]
        def race(p):
            try:
                call(f"/posts/{p['id']}/schedule", {"version": p["version"], "channel_ids": [channel["id"]], "scheduled_at": later})
                return 200
            except AssertionError as error:
                assert error.args[0][1] == 409, error
                return 409
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            assert sorted(pool.map(race, racing)) == [200, 409]
        due = call(f"/posts/{post['id']}/schedule", {"version": post["version"], "channel_ids": [channel["id"]], "scheduled_at": dt.datetime.now(dt.timezone.utc).isoformat()})[0]
        wait_state(due, "manual_required")
        call(f"/publications/{due['id']}/manual-complete", {"note": "Isolated test; no network post"})
        attempts = call(f"/publications/{due['id']}/attempts")
        assert any(a.get("receipt", {}).get("verification") == "owner_attestation" for a in attempts)
        call(f"/publications/{due['id']}/retry", {}, expected=409)
        call(f"/posts/{post['id']}", method="DELETE", expected=409)
        call("/schedules", {"channel_id": channel["id"], "days": [1, 2, 3, 4, 5, 6, 7], "time": "09:00", "timezone": "America/Chicago"})
        scheduled = call("/posts", {"brand_id": brand["id"], "text": "Next local slot"})
        next_slot = call(f"/posts/{scheduled['id']}/schedule", {"version": scheduled["version"], "channel_ids": [channel["id"]]})[0]
        assert dt.datetime.fromisoformat(next_slot["scheduled_at"].replace("Z", "+00:00")).astimezone(ZoneInfo("America/Chicago")).hour == 9
        queue_post = call("/posts", {"brand_id": brand["id"], "text": "Second queue slot"})
        queue_two = call(f"/posts/{queue_post['id']}/schedule", {"version": queue_post["version"], "channel_ids": [channel["id"]]})[0]
        assert next_slot["scheduling_mode"] == queue_two["scheduling_mode"] == "queue"
        fixed = next(p for p in call("/publications") if p["state"] == "queued" and p["scheduling_mode"] == "exact")
        call("/queue/reorder", {"channel_id": channel["id"], "publication_ids": [queue_two["id"], next_slot["id"], fixed["id"]]}, expected=409)
        reordered = call("/queue/reorder", {"channel_id": channel["id"], "publication_ids": [queue_two["id"], next_slot["id"]]})
        assert reordered[0]["scheduled_at"] == next_slot["scheduled_at"]
        assert next(p for p in call("/publications") if p["id"] == fixed["id"])["scheduled_at"] == fixed["scheduled_at"]
        call(f"/channels/{channel['id']}", {"paused": True}, "PATCH")
        paused = call("/posts", {"brand_id": brand["id"], "text": "Paused"})
        pending = call(f"/posts/{paused['id']}/schedule", {"version": paused["version"], "channel_ids": [channel["id"]], "scheduled_at": dt.datetime.now(dt.timezone.utc).isoformat()})[0]
        time.sleep(2.5)
        assert next(p for p in call("/publications") if p["id"] == pending["id"])["state"] == "queued"
        sql(f"UPDATE publications SET state='sending',lease_until=now()-interval '1 minute' WHERE id='{pending['id']}'; INSERT INTO publication_attempts(id,publication_id,state,provider) VALUES(gen_random_uuid(),'{pending['id']}','sending','manual');")
        wait_state(pending, "uncertain")
        call(f"/publications/{pending['id']}/retry", {}, expected=409)
        call(f"/publications/{pending['id']}/reconcile", {"resolution": "not_published", "note": "No external send in this isolated fixture"})
        assert call(f"/publications/{pending['id']}/retry", {})["state"] == "queued"
        # Stop a real worker, leave due work durable, interrupt its next attempt, and restart two workers.
        original_worker = processes[1]
        original_worker.send_signal(signal.SIGTERM)
        original_worker.wait(timeout=10)
        fixture = call("/channels", {"brand_id": brand["id"], "name": "Restart fixture", "provider": "fixture"})
        crash_post = call("/posts", {"brand_id": brand["id"], "text": "Durable during worker stop"})
        crash_job = call(f"/posts/{crash_post['id']}/schedule", {"version": crash_post["version"], "channel_ids": [fixture["id"]], "scheduled_at": dt.datetime.now(dt.timezone.utc).isoformat()})[0]
        time.sleep(0.5)
        assert next(p for p in call("/publications") if p["id"] == crash_job["id"])["state"] == "queued"
        slow_worker = subprocess.Popen([str(ROOT / "target/debug/beweft-worker")], cwd=ROOT, env=dict(env, BEWEFT_FIXTURE_DELAY_MS="60000"), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        processes.append(slow_worker)
        wait_state(crash_job, "sending")
        heartbeat_before = call("/jobs/health")["worker_last_seen"]
        time.sleep(11)
        heartbeat_after = call("/jobs/health")
        assert heartbeat_after["worker_healthy"] and heartbeat_after["worker_last_seen"] > heartbeat_before, "Heartbeat must advance during a slow in-flight operation"
        slow_worker.kill()  # Intentional isolated process crash, not a simulated row transition.
        slow_worker.wait(timeout=10)
        assert next(p for p in call("/publications") if p["id"] == crash_job["id"])["state"] == "sending"
        # Accelerate only the lease clock rather than waiting five minutes after the actual crash.
        sql(f"UPDATE publications SET lease_until=now()-interval '1 second' WHERE id='{crash_job['id']}';")
        for _ in range(2):
            processes.append(subprocess.Popen([str(ROOT / "target/debug/beweft-worker")], cwd=ROOT, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        wait_state(crash_job, "uncertain")
        assert len(call(f"/publications/{crash_job['id']}/attempts")) == 1, "Restart must not create a second ambiguous attempt"
        batch = []
        for index in range(8):
            item = call("/posts", {"brand_id": brand["id"], "text": f"Two-worker fixture {index}"})
            batch.append(call(f"/posts/{item['id']}/schedule", {"version": item["version"], "channel_ids": [fixture["id"]], "scheduled_at": dt.datetime.now(dt.timezone.utc).isoformat()})[0])
        for job in batch:
            wait_state(job, "fixture_complete")
            attempts = call(f"/publications/{job['id']}/attempts")
            assert len(attempts) == 1 and attempts[0]["receipt"]["external_publication"] is False
        # Queue a pause before eligible worker claims on the same advisory lock: pause must win.
        pause_channel = call("/channels", {"brand_id": brand["id"], "name": "Pause race", "provider": "manual"})
        pause_post = call("/posts", {"brand_id": brand["id"], "text": "Must remain queued after pause wins"})
        pause_job = call(f"/posts/{pause_post['id']}/schedule", {"version": pause_post["version"], "channel_ids": [pause_channel["id"]], "scheduled_at": (dt.datetime.now(dt.timezone.utc) + dt.timedelta(seconds=2)).isoformat()})[0]
        holder = subprocess.Popen(["docker", "exec", "-i", CONTAINER, "psql", "-U", DB_USER, "-d", DB, "-Atq", "-v", "ON_ERROR_STOP=1"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            holder.stdin.write(f"BEGIN; SELECT pg_advisory_xact_lock(hashtextextended('{pause_channel['id']}',0)); SELECT 'LOCKED';\n")
            holder.stdin.flush()
            while holder.stdout.readline().strip() != "LOCKED":
                assert holder.poll() is None
            with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
                pause_future = pool.submit(call, f"/channels/{pause_channel['id']}", {"paused": True}, "PATCH")
                try:
                    for _ in range(50):
                        waiting = int(sql("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event='advisory';").stdout.splitlines()[2].strip())
                        if waiting:
                            break
                        time.sleep(0.05)
                    assert not pause_future.done(), "Pause bypassed the shared channel claim lock"
                    time.sleep(2.5)  # The two workers become eligible and queue behind the waiting pause.
                finally:
                    holder.stdin.write("COMMIT; SELECT 'RELEASED';\n")
                    holder.stdin.flush()
                assert pause_future.result(timeout=10)["paused"] is True
            time.sleep(2.5)
            assert next(p for p in call("/publications") if p["id"] == pause_job["id"])["state"] == "queued"
            assert call(f"/publications/{pause_job['id']}/attempts") == []
        finally:
            if holder.poll() is None:
                holder.stdin.close()
                holder.wait(timeout=10)

        # ABA regression with two real workers: old A returns while a fresh retry B is sending.
        for process in processes[1:]:
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
                process.wait(timeout=10)
        aba_channel = call("/channels", {"brand_id": brand["id"], "name": "Attempt identity race", "provider": "fixture"})
        aba_post = call("/posts", {"brand_id": brand["id"], "text": "Old attempt must never complete a new retry"})
        aba_job = call(f"/posts/{aba_post['id']}/schedule", {"version": aba_post["version"], "channel_ids": [aba_channel["id"]], "scheduled_at": dt.datetime.now(dt.timezone.utc).isoformat()})[0]
        worker_a = subprocess.Popen([str(ROOT / "target/debug/beweft-worker")], cwd=ROOT, env=dict(env, BEWEFT_FIXTURE_DELAY_MS="6000"), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        processes.append(worker_a)
        wait_state(aba_job, "sending")
        attempt_a = next(a for a in call(f"/publications/{aba_job['id']}/attempts") if a["state"] == "sending")
        sql(f"UPDATE publications SET lease_until=now()-interval '1 second' WHERE id='{aba_job['id']}';")
        worker_b = subprocess.Popen([str(ROOT / "target/debug/beweft-worker")], cwd=ROOT, env=dict(env, BEWEFT_FIXTURE_DELAY_MS="15000"), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        processes.append(worker_b)
        wait_state(aba_job, "uncertain")
        call(f"/publications/{aba_job['id']}/reconcile", {"resolution": "not_published", "note": "Fixture A has no external publication; deliberate attempt fencing reproduction"})
        call(f"/publications/{aba_job['id']}/retry", {})
        wait_state(aba_job, "sending")
        attempt_b = next(a for a in call(f"/publications/{aba_job['id']}/attempts") if a["state"] == "sending")
        assert attempt_a["id"] != attempt_b["id"]
        # Signal A while it sleeps. It finishes its in-flight attempt, then exits, proving its result was processed.
        worker_a.send_signal(signal.SIGTERM)
        worker_a.wait(timeout=10)
        publication_after_a = next(p for p in call("/publications") if p["id"] == aba_job["id"])
        attempts_after_a = call(f"/publications/{aba_job['id']}/attempts")
        assert publication_after_a["state"] == "sending", "Old A overwrote B's current publication state"
        assert next(a for a in attempts_after_a if a["id"] == attempt_b["id"])["state"] == "sending"
        assert next(a for a in attempts_after_a if a["id"] == attempt_a["id"])["state"] == "uncertain"
        for _ in range(100):
            if next(p for p in call("/publications") if p["id"] == aba_job["id"])["state"] == "fixture_complete":
                break
            time.sleep(0.2)
        else:
            raise AssertionError("Current attempt B did not complete")
        assert next(a for a in call(f"/publications/{aba_job['id']}/attempts") if a["id"] == attempt_b["id"])["state"] == "fixture_complete"
        # Run stored-rule DST integration with a controlled explicit clock against this same isolated database.
        subprocess.run(["cargo", "test", "-q", "-p", "beweft-server", "--test", "scheduling", "--", "--ignored"], cwd=ROOT, env=dict(env, BEWEFT_TEST_DATABASE_URL=env["DATABASE_URL"]), check=True)
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
        # Stream a video-sized synthetic fixture beyond the old 25 MiB limit without retaining a large file.
        limits = call("/media/limits")
        assert limits["max_upload_bytes"] == 32 * 1024 * 1024 and limits["quota_bytes"] == 33 * 1024 * 1024
        upload_token = call("/tokens", {"name": "Streaming fixture", "scopes": ["drafts:write"]})["token"]
        large_size = 26 * 1024 * 1024
        prefix = b'--large-fixture\r\nContent-Disposition: form-data; name="file"; filename="synthetic.mp4"\r\nContent-Type: video/mp4\r\n\r\n'
        suffix = b'\r\n--large-fixture--\r\n'
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=20)
        connection.putrequest("POST", "/api/v1/media?brand_id=" + brand["id"])
        connection.putheader("Authorization", "Bearer " + upload_token)
        connection.putheader("Content-Type", "multipart/form-data; boundary=large-fixture")
        connection.putheader("Content-Length", str(len(prefix) + large_size + len(suffix)))
        connection.endheaders()
        connection.send(prefix)
        header = b'\x00\x00\x00\x18ftypisom'
        connection.send(header)
        remaining = large_size - len(header)
        chunk = b'0' * 65536
        while remaining:
            amount = min(remaining, len(chunk))
            connection.send(chunk[:amount])
            remaining -= amount
        connection.send(suffix)
        response = connection.getresponse()
        assert response.status == 200, response.read()
        large_media = json.loads(response.read())["data"]
        connection.close()
        assert large_media["size"] == large_size
        assert call("/media/limits")["used_bytes"] == large_size
        # Quota admission accounts for in-flight reservations before any file is accepted.
        # A declared 8 MiB request cannot fit the remaining 7 MiB, so it is rejected immediately.
        denied = http.client.HTTPConnection("127.0.0.1", port, timeout=20)
        denied.putrequest("POST", "/api/v1/media?brand_id=" + brand["id"])
        denied.putheader("Authorization", "Bearer " + upload_token)
        denied.putheader("Content-Type", "multipart/form-data; boundary=large-fixture")
        denied.putheader("Content-Length", str(8 * 1024 * 1024))
        denied.endheaders()
        denied.send(prefix + header)
        response = denied.getresponse()
        assert response.status == 400 and b"quota" in response.read().lower()
        denied.close()
        call(f"/media/{large_media['id']}", method="DELETE")
        # Abort a partially streamed upload: its temporary file and quota reservation must be reclaimed.
        interrupted = http.client.HTTPConnection("127.0.0.1", port, timeout=20)
        interrupted.putrequest("POST", "/api/v1/media?brand_id=" + brand["id"])
        interrupted.putheader("Authorization", "Bearer " + upload_token)
        interrupted.putheader("Content-Type", "multipart/form-data; boundary=large-fixture")
        interrupted.putheader("Content-Length", str(1024 * 1024))
        interrupted.endheaders()
        interrupted.send(prefix + header + chunk)
        for _ in range(50):
            if call("/media/limits")["reserved_bytes"] > 0:
                break
            time.sleep(0.1)
        else:
            raise AssertionError("Partial upload did not enter quota reservation")
        interrupted.close()
        for _ in range(30):
            if call("/media/limits")["reserved_bytes"] == 0 and not list(Path(temporary).iterdir()):
                break
            time.sleep(0.1)
        assert call("/media/limits")["reserved_bytes"] == 0
        assert not list(Path(temporary).iterdir()), "Deleted media file was retained"
        assert call("/jobs/health")["worker_healthy"]
        assert call("/analytics")["availability"] == "unavailable"
        assert call("/inbox")["availability"] == "unavailable"
        exercise(call, client, origin, sql)
        # Apply the additive migration to the previous schema and populated records, then roll back the isolated schema.
        upgrade_schema = "upgrade_" + uuid.uuid4().hex[:12]
        old_brand, old_channel, old_post, old_publication, old_media = [str(uuid.uuid4()) for _ in range(5)]
        upgrade_sql = f"BEGIN; CREATE SCHEMA {upgrade_schema}; SET LOCAL search_path TO {upgrade_schema};\n" + (ROOT / "migrations/0001_foundation.sql").read_text()
        upgrade_sql += f"\nINSERT INTO brands(id,name) VALUES('{old_brand}','Upgrade fixture'); INSERT INTO channels(id,brand_id,name,provider) VALUES('{old_channel}','{old_brand}','Old channel','manual'); INSERT INTO posts(id,brand_id,text) VALUES('{old_post}','{old_brand}','Existing draft'); INSERT INTO publications(id,post_id,brand_id,channel_id,post_version,scheduled_at,state,payload) VALUES('{old_publication}','{old_post}','{old_brand}','{old_channel}',1,'2030-01-02T12:34:00Z','queued','{{\"text\":\"Existing snapshot\",\"media_ids\":[]}}');\n"
        upgrade_sql += f"\nINSERT INTO media(id,brand_id,name,mime,size,storage_key) VALUES('{old_media}','{old_brand}','Existing file','image/png',1,'existing.bin');\n"
        upgrade_sql += (ROOT / "migrations/0003_scheduling_mode.sql").read_text()
        upgrade_sql += (ROOT / "migrations/0005_workflows.sql").read_text()
        upgrade_sql += (ROOT / "migrations/0010_media_metadata_version.sql").read_text()
        upgrade_sql += f"\nDO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM media WHERE id='{old_media}' AND name='Existing file' AND storage_key='existing.bin' AND alt_text='' AND metadata_version=1) THEN RAISE EXCEPTION 'Media upgrade changed existing file'; END IF; END $$;"
        upgrade_sql += f"\nDO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM channels WHERE id='{old_channel}' AND target_platform='generic') OR NOT EXISTS(SELECT 1 FROM brands WHERE id='{old_brand}' AND archived_at IS NULL) THEN RAISE EXCEPTION 'Workflow migration changed old brand/channel semantics'; END IF; END $$;"
        upgrade_sql += f"\nDO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM publications WHERE id='{old_publication}' AND scheduling_mode='exact' AND scheduled_at='2030-01-02T12:34:00Z' AND payload->>'text'='Existing snapshot') THEN RAISE EXCEPTION 'Upgrade changed existing publication'; END IF; END $$; ROLLBACK;"
        subprocess.run(["docker", "exec", "-i", CONTAINER, "psql", "-U", DB_USER, "-d", DB, "-v", "ON_ERROR_STOP=1"], input=upgrade_sql, text=True, capture_output=True, check=True)
        print("PASS: real HTTP, PostgreSQL and worker auth/CSRF/scoped access, immutable revisions, concurrent reservation, manual handoff, exact-date preservation, real stop/crash/restart, two workers, live heartbeat during slow work, stored-rule DST, streamed26MiB upload, quota/admission/cancellation cleanup, typed OpenAPI response validation, populated-schema migration upgrade, old-attempt ABA fencing, ordered pause/claim, session-only admin-token rejection")


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
