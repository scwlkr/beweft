#!/usr/bin/env python3
"""Real HTTP/PostgreSQL scheduling idempotency regression; uses no provider services.

Build the server and set CARGO_TARGET_DIR to the build directory before running.
Creates and removes one disposable database and media directory.
"""
import concurrent.futures
import datetime as dt
import http.cookiejar
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[3]
CONTAINER = os.environ.get("BEWEFT_TEST_POSTGRES_CONTAINER", "beweft-dev-postgres")
USER = os.environ.get("BEWEFT_TEST_DB_USER", "beweft")
PASSWORD = os.environ.get("BEWEFT_TEST_DB_PASSWORD", "beweft-local-development")
PORT = os.environ.get("BEWEFT_TEST_DB_PORT", "55439")
DB = "beweft_schedule_test_" + uuid.uuid4().hex[:12]
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target"))).resolve()


def sql(query):
    return subprocess.run(
        ["docker", "exec", CONTAINER, "psql", "-X", "-qAt", "-U", USER,
         "-d", DB, "-v", "ON_ERROR_STOP=1", "-c", query],
        check=True, capture_output=True, text=True,
    ).stdout.strip()


def timestamp(delta=dt.timedelta(hours=1)):
    return (dt.datetime.now(dt.timezone.utc) + delta).isoformat(timespec="microseconds")


def run():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    origin = f"http://127.0.0.1:{port}"
    process = None
    locker = None
    with tempfile.TemporaryDirectory(prefix="beweft-schedule-fixture-") as media:
        env = dict(os.environ, DATABASE_URL=f"postgres://{USER}:{PASSWORD}@localhost:{PORT}/{DB}",
                   BEWEFT_PUBLIC_URL=origin, BEWEFT_BIND=f"127.0.0.1:{port}", BEWEFT_MEDIA_DIR=media,
                   BEWEFT_SETUP_SECRET="isolated-schedule-test-secret", BEWEFT_ENABLE_FIXTURES="true")
        process = subprocess.Popen([str(TARGET / "debug/beweft-server")], cwd=ROOT, env=env,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

        def response(path, body=None, method=None, token=None):
            headers = {"X-Beweft-Request": "1", "Origin": origin}
            if body is not None:
                headers["Content-Type"] = "application/json"
            if token:
                headers["Authorization"] = "Bearer " + token
            request = urllib.request.Request(origin + "/api/v1" + path,
                data=None if body is None else json.dumps(body).encode(), headers=headers,
                method=method or ("POST" if body is not None else "GET"))
            try:
                with client.open(request, timeout=20) as result:
                    return result.status, result.read()
            except urllib.error.HTTPError as error:
                return error.code, error.read()

        def call(path, body=None, method=None, expected=200, token=None):
            status, raw = response(path, body, method, token)
            assert status == expected, (path, status, expected, raw[:400])
            value = json.loads(raw)
            return value["data"] if status == 200 else value

        try:
            for _ in range(100):
                try:
                    if response("/auth/status")[0] == 200:
                        break
                except OSError:
                    assert process.poll() is None, "server exited during startup"
                    time.sleep(0.1)
            else:
                raise AssertionError("server did not start")
            call("/auth/bootstrap", {"setup_secret": env["BEWEFT_SETUP_SECRET"],
                                      "email": "schedule@example.invalid", "password": "Isolated-schedule-password!"})
            brand = call("/brands", {"name": "Schedule fixture"})["id"]
            other_brand = call("/brands", {"name": "Other fixture"})["id"]
            channels = [call("/channels", {"brand_id": brand, "name": f"Manual {i}", "provider": "manual"})["id"] for i in range(2)]
            other_channel = call("/channels", {"brand_id": other_brand, "name": "Other", "provider": "manual"})["id"]
            post = call("/posts", {"brand_id": brand, "text": "Original intent"})
            key = str(uuid.uuid4())
            when = timestamp()
            alternate = dt.datetime.fromisoformat(when).astimezone(dt.timezone(dt.timedelta(hours=5, minutes=30))).isoformat(timespec="microseconds")
            body = {"version": 1, "channel_ids": channels, "scheduled_at": when, "request_id": key}
            equivalent = {**body, "channel_ids": channels[::-1], "scheduled_at": alternate}
            path = f"/posts/{post['id']}/schedule?view=summary"
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                concurrent_results = list(pool.map(lambda data: response(path, data), [body, equivalent]))
            assert any(status == 200 for status, _ in concurrent_results)
            for status, raw in concurrent_results:
                assert status in (200, 409), (status, raw[:400])
                if status == 409:
                    assert json.loads(raw)["error"]["code"] == "schedule_request_in_progress"
            first, second = call(path, body), call(path, equivalent)
            assert [x["id"] for x in first] == [x["id"] for x in second]
            assert sql(f"SELECT count(*) FROM publications WHERE post_id='{post['id']}'") == "2"
            assert [x["channel_id"] for x in first] == sorted(channels)
            result = call(f"/schedule-requests/{key}")
            assert result["request_id"] == key and result["post_id"] == post["id"] and result["brand_id"] == brand
            assert result["request"] == {"version": 1, "channel_ids": sorted(channels), "scheduled_at": when.replace("+00:00", "Z")}
            assert [x["id"] for x in result["publications"]] == [x["id"] for x in first]
            assert all("payload" not in x for x in result["publications"])
            assert call(path, {**body, "version": 2}, expected=409)["error"]["code"] == "schedule_request_conflict"
            assert call(path, {**body, "channel_ids": channels[:1]}, expected=409)["error"]["code"] == "schedule_request_conflict"
            assert call(path, {**body, "scheduled_at": timestamp(dt.timedelta(hours=3))}, expected=409)["error"]["code"] == "schedule_request_conflict"
            different = call("/posts", {"brand_id": brand, "text": "Different post"})
            assert call(f"/posts/{different['id']}/schedule", body, expected=409)["error"]["code"] == "schedule_request_conflict"

            # Queue-mode intent normalizes omitted and explicit null times,
            # while response projection remains outside the request identity.
            for channel in channels:
                call("/schedules", {"channel_id": channel, "days": [1, 2, 3, 4, 5, 6, 7], "time": "09:00", "timezone": "UTC"})
            queue_post = call("/posts", {"brand_id": brand, "text": "Queued intent"})
            queue_key = str(uuid.uuid4())
            queue_body = {"version": 1, "channel_ids": channels, "request_id": queue_key}
            queued = call(f"/posts/{queue_post['id']}/schedule?view=summary", queue_body)
            queue_replay = call(f"/posts/{queue_post['id']}/schedule", {**queue_body, "channel_ids": channels[::-1], "scheduled_at": None})
            assert [p["id"] for p in queued] == [p["id"] for p in queue_replay]
            assert call(f"/schedule-requests/{queue_key}")["request"]["scheduled_at"] is None
            assert all("payload" in p for p in queue_replay)

            # A consumed response body is deliberately discarded after commit.
            lost = call("/posts", {"brand_id": brand, "text": "Lost response"})
            lost_key = str(uuid.uuid4())
            lost_body = {"version": 1, "channel_ids": [channels[0]], "scheduled_at": timestamp(dt.timedelta(hours=4)), "request_id": lost_key}
            request = urllib.request.Request(origin + f"/api/v1/posts/{lost['id']}/schedule?view=summary", data=json.dumps(lost_body).encode(),
                headers={"X-Beweft-Request": "1", "Origin": origin, "Content-Type": "application/json"})
            with client.open(request, timeout=10) as committed:
                assert committed.status == 200
                # No publication identities are read from this response.
            recovered = call(f"/schedule-requests/{lost_key}")
            replay = call(f"/posts/{lost['id']}/schedule?view=summary", lost_body)
            assert [p["id"] for p in recovered["publications"]] == [p["id"] for p in replay]

            # Hold the original on its channel lock. More than ten same-key
            # retries must fail promptly without leaving database lock waiters
            # behind or exhausting the ten-connection application pool.
            pending_post = call("/posts", {"brand_id": brand, "text": "In flight"})
            pending_key = str(uuid.uuid4())
            pending_body = {"version": 1, "channel_ids": [channels[0]], "scheduled_at": timestamp(dt.timedelta(hours=5)), "request_id": pending_key}
            locker = subprocess.Popen(["docker", "exec", "-i", CONTAINER, "psql", "-X", "-qAt", "-U", USER, "-d", DB,
                                       "-v", "ON_ERROR_STOP=1"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            locker.stdin.write(f"BEGIN; SELECT pg_advisory_xact_lock(hashtextextended('{channels[0]}',0)); SELECT 'held';\n")
            locker.stdin.flush()
            while locker.stdout.readline().strip() != "held":
                assert locker.poll() is None, "fixture lock failed"
            pending_path = f"/posts/{pending_post['id']}/schedule?view=summary"
            with concurrent.futures.ThreadPoolExecutor(max_workers=14) as pool:
                original = pool.submit(call, pending_path, pending_body)
                for _ in range(100):
                    waiting = sql("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%pg_advisory_xact_lock%')")
                    if waiting == "t":
                        break
                    time.sleep(0.05)
                else:
                    raise AssertionError("schedule did not wait on the fixture channel lock")
                call(f"/schedule-requests/{pending_key}", expected=404)
                started = time.monotonic()
                retries = [pool.submit(call, pending_path, pending_body, expected=409) for _ in range(12)]
                for retry in retries:
                    assert retry.result(timeout=3)["error"]["code"] == "schedule_request_in_progress"
                assert time.monotonic() - started < 3, "same-key retries did not fail promptly"
                for _ in range(3):
                    assert call(pending_path, pending_body, expected=409)["error"]["code"] == "schedule_request_in_progress"
                # Auth and the receipt endpoint still have DB capacity while the
                # original owns the request key and remains blocked on channel.
                started = time.monotonic()
                assert call("/auth/me")["email"] == "schedule@example.invalid"
                call(f"/schedule-requests/{pending_key}", expected=404)
                assert time.monotonic() - started < 2, "receipt or auth starved"
                waiters = sql("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%pg_advisory_xact_lock%'")
                assert waiters == "1", ("retry queries remained blocked", waiters)
                # The previous timed-out queries remained on PostgreSQL for
                # more than seven seconds. Confirm that none persist later.
                time.sleep(7.2)
                assert call("/auth/me")["email"] == "schedule@example.invalid"
                call(f"/schedule-requests/{pending_key}", expected=404)
                waiters = sql("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%pg_advisory_xact_lock%'")
                assert waiters == "1", ("retry queries remained blocked", waiters)
                assert not original.done()
                locker.stdin.write("COMMIT;\n\\q\n")
                locker.stdin.flush()
                locker.wait(timeout=5)
                locker = None
                first = original.result(timeout=10)
            second = call(pending_path, pending_body)
            assert [x["id"] for x in first] == [x["id"] for x in second]
            assert [x["id"] for x in call(f"/schedule-requests/{pending_key}")["publications"]] == [x["id"] for x in first]
            assert sql(f"SELECT count(*) FROM publications WHERE post_id='{pending_post['id']}'") == "1"

            # Cancellation/replacement and a newer draft cannot change which
            # publication IDs the original intent returns.
            changed = call(f"/posts/{post['id']}", {"version": 1, "text": "Changed"}, method="PATCH")
            replaced = call(f"/publications/{first_id(result, channels[0])}/replace", {
                "post_version": changed["version"], "publication_post_version": 1, "scheduled_at": when})
            remaining = first_id(result, channels[1])
            call(f"/publications/{remaining}/cancel", {})
            current = call(path, body)
            assert [p["id"] for p in current] == [p["id"] for p in result["publications"]]
            assert all(p["state"] == "canceled" for p in current)
            assert replaced["publication"]["id"] not in [p["id"] for p in current]

            # Unauthorized lookups are indistinguishable from missing ones.
            token = call("/tokens", {"name": "Other brand only", "scopes": ["read", "publishing:write"], "brand_ids": [other_brand]})["token"]
            forbidden = call(f"/schedule-requests/{key}", expected=404, token=token)
            missing = call(f"/schedule-requests/{uuid.uuid4()}", expected=404, token=token)
            assert forbidden == missing
            other_post = call("/posts", {"brand_id": other_brand, "text": "Other"})
            call(f"/posts/{other_post['id']}/schedule", {"version": 1, "channel_ids": [other_channel], "scheduled_at": when, "request_id": key}, expected=404, token=token)

            # The same UUID belongs to a separate owner namespace; neither
            # owner's lookup exposes the other's committed request.
            owner_two = str(uuid.uuid4())
            owner_token = "bw_fixture_" + uuid.uuid4().hex
            token_digest = hashlib.sha256(owner_token.encode()).hexdigest()
            sql(f"INSERT INTO owners(id,email,password_hash) VALUES('{owner_two}','owner2@example.invalid','unused'); "
                f"INSERT INTO api_tokens(id,owner_id,name,token_hash,scopes,brand_ids) VALUES(gen_random_uuid(),'{owner_two}','fixture','{token_digest}',"
                f"'[\"read\",\"publishing:write\"]','[\"{other_brand}\"]')")
            call(f"/schedule-requests/{key}", expected=404, token=owner_token)
            owner_two_body = {"version": 1, "channel_ids": [other_channel], "scheduled_at": when, "request_id": key}
            owner_two_result = call(f"/posts/{other_post['id']}/schedule?view=summary", owner_two_body, token=owner_token)
            assert call(f"/schedule-requests/{key}", token=owner_token)["post_id"] == other_post["id"]
            assert call(f"/schedule-requests/{key}")["post_id"] == post["id"]
            assert owner_two_result[0]["post_id"] == other_post["id"]

            # A partially constructed request must roll back both publications
            # and its request key, which is then safe to reuse for a valid intent.
            partial = call("/posts", {"brand_id": brand, "text": "Partial rollback"})
            partial_key = str(uuid.uuid4())
            partial_body = {"version": 1, "channel_ids": [channels[0], "ffffffff-ffff-4fff-bfff-ffffffffffff"],
                            "scheduled_at": timestamp(dt.timedelta(hours=6)), "request_id": partial_key}
            call(f"/posts/{partial['id']}/schedule", partial_body, expected=400)
            call(f"/schedule-requests/{partial_key}", expected=404)
            assert sql(f"SELECT count(*) FROM publications WHERE post_id='{partial['id']}'") == "0"
            accepted = call(f"/posts/{partial['id']}/schedule?view=summary", {**partial_body, "channel_ids": [channels[0]]})
            assert len(accepted) == 1

            # Timestamp parsing rejects any seventh fractional digit before
            # mutation, including trailing zeroes that a parser might truncate.
            precise = call("/posts", {"brand_id": brand, "text": "Precision"})
            precise_key = str(uuid.uuid4())
            raw_time = timestamp(dt.timedelta(hours=7)).replace("+00:00", "0Z")
            precise_body = {"version": 1, "channel_ids": [channels[0]], "scheduled_at": raw_time, "request_id": precise_key}
            call(f"/posts/{precise['id']}/schedule", precise_body, expected=422)
            call(f"/schedule-requests/{precise_key}", expected=404)
            assert sql(f"SELECT count(*) FROM publications WHERE post_id='{precise['id']}'") == "0"

            past = call("/posts", {"brand_id": brand, "text": "Time passes"})
            past_key = str(uuid.uuid4())
            past_body = {"version": 1, "channel_ids": [channels[0]], "scheduled_at": timestamp(dt.timedelta(seconds=-4)), "request_id": past_key}
            made = call(f"/posts/{past['id']}/schedule?view=summary", past_body)
            time.sleep(2.2)
            assert call(f"/posts/{past['id']}/schedule?view=summary", past_body)[0]["id"] == made[0]["id"]
            call(f"/posts/{past['id']}/schedule", {**past_body, "request_id": str(uuid.uuid4())}, expected=400)

            # Existing provider-derived rows can contain oversized display
            # names and unknown provider labels regardless of API input limits.
            # Use escaped controls and multibyte characters to test the bound.
            bulk_brand = call("/brands", {"name": "Bounded recovery"})["id"]
            bulk_channels = [call("/channels", {"brand_id": bulk_brand, "name": f"Bulk {i}", "provider": "manual"})["id"] for i in range(50)]
            sql(f"UPDATE channels SET name=repeat(chr(1),100000), provider=repeat(chr(2),100000), target_platform=NULL WHERE brand_id='{bulk_brand}'")
            sql(f"UPDATE channels SET name=repeat('é😊',50000), provider=repeat('界🪐',50000) WHERE id='{bulk_channels[0]}'")
            bulk_post = call("/posts", {"brand_id": bulk_brand, "text": "\u0001" * 100000})
            bulk_key = str(uuid.uuid4())
            bulk_body = {"version": 1, "channel_ids": bulk_channels, "scheduled_at": timestamp(dt.timedelta(hours=8)), "request_id": bulk_key}
            status, raw = response(f"/posts/{bulk_post['id']}/schedule?view=summary", bulk_body)
            assert status == 200 and len(raw) < 100000
            bulk = json.loads(raw)["data"]
            assert len(bulk) == 50 and [row["channel_id"] for row in bulk] == sorted(bulk_channels)
            assert all(len(row["channel_name"]) == 100 and len(row["provider"]) == 64 for row in bulk)
            unicode_row = next(row for row in bulk if row["channel_id"] == bulk_channels[0])
            assert unicode_row["channel_name"] == "é😊" * 50 and unicode_row["provider"] == "界🪐" * 32
            status, raw = response(f"/schedule-requests/{bulk_key}")
            assert status == 200 and len(raw) < 100000
            receipt = json.loads(raw)["data"]
            assert receipt["request"]["channel_ids"] == sorted(bulk_channels)
            assert receipt["publications"] == bulk
            status, raw = response(f"/posts/{bulk_post['id']}/schedule?view=summary", bulk_body)
            assert status == 200 and len(raw) < 100000 and json.loads(raw)["data"] == bulk
            pages = []
            cursor = None
            while True:
                query = {"brand_id": bulk_brand, "limit": 10, "view": "summary"}
                if cursor is not None:
                    query["cursor"] = cursor
                status, raw = response("/publications?" + urllib.parse.urlencode(query))
                assert status == 200 and len(raw) < 30000
                page = json.loads(raw)
                pages.extend(page["data"])
                cursor = page["next_cursor"]
                if cursor is None:
                    break
            assert {row["id"] for row in pages} == {row["id"] for row in bulk}
            assert {row["id"]: row for row in pages} == {row["id"]: row for row in bulk}
            # The single-record full detail still returns the original values.
            control_row = next(row for row in bulk if row["channel_id"] != bulk_channels[0])
            full = call(f"/publications?id={control_row['id']}&limit=1")[0]
            assert full["channel_name"] == "\u0001" * 100000
            assert full["provider"] == "\u0002" * 100000

            # The exported contract includes both full and compact paged
            # representations and the runtime emits the described fields.
            with client.open(origin + "/api-docs/openapi.json", timeout=5) as schema_response:
                schema = json.load(schema_response)
            paths = schema["paths"]
            for route in ["/api/v1/posts", "/api/v1/media", "/api/v1/publications"]:
                operation = paths[route]["get"]
                assert {"limit", "cursor"}.issubset({p["name"] for p in operation["parameters"]})
                variants = operation["responses"]["200"]["content"]["application/json"]["schema"]["anyOf"]
                assert any("next_cursor" in item["required"] for item in variants)
            assert "PostSummary" in json.dumps(paths["/api/v1/posts"]["get"])
            assert "PublicationSummary" in json.dumps(paths["/api/v1/publications"]["get"])
            assert "PublicationSummary" in json.dumps(paths["/api/v1/posts/{id}/schedule"]["post"])
            assert any(p["name"] == "request_id" for p in paths["/api/v1/schedule-requests/{request_id}"]["get"]["parameters"])
            assert schema["components"]["schemas"]["ScheduleRequestIntent"]["required"] == ["version", "channel_ids", "scheduled_at"]
            summary_schema = schema["components"]["schemas"]["PublicationSummary"]["properties"]
            assert summary_schema["channel_name"]["maxLength"] == 100
            assert summary_schema["provider"]["maxLength"] == 64
            for route in ["posts", "publications"]:
                status, raw = response(f"/{route}?limit=1&view=summary")
                value = json.loads(raw)
                assert status == 200 and "next_cursor" in value and len(value["data"]) == 1
                assert "payload" not in value["data"][0] and "variants" not in value["data"][0]
            assert json.dumps(schema, sort_keys=True) == json.dumps(json.loads((ROOT / "docs/openapi.json").read_text()), sort_keys=True)
            exported_ts = subprocess.run([str(TARGET / "debug/beweft-contract"), "--typescript"], check=True, capture_output=True, text=True).stdout
            assert exported_ts == (ROOT / "web/src/lib/api-contract.generated.ts").read_text()

            # Archive blocks new work but not an authorized replay of old work.
            call(f"/brands/{brand}", {"archived": True}, method="PATCH")
            assert [p["id"] for p in call(path, body)] == [p["id"] for p in result["publications"]]
            call(f"/posts/{different['id']}/schedule", {"version": 1, "channel_ids": [channels[0]], "scheduled_at": timestamp(), "request_id": str(uuid.uuid4())}, expected=409)
            print("PASS bounded summary names, repeated same-key retries and scheduling recovery invariants")
        finally:
            if locker is not None:
                locker.stdin.write("ROLLBACK;\n\\q\n")
                locker.stdin.flush()
                locker.wait(timeout=5)
            if process is not None:
                process.terminate()
                process.wait(timeout=10)


def first_id(result, channel):
    return next(p["id"] for p in result["publications"] if p["channel_id"] == channel)


if __name__ == "__main__":
    created = False
    try:
        subprocess.run(["docker", "exec", CONTAINER, "createdb", "-U", USER, DB], check=True)
        created = True
        run()
    finally:
        if created:
            subprocess.run(["docker", "exec", CONTAINER, "dropdb", "-U", USER, "--force", DB], check=True)
