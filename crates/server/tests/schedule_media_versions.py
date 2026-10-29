#!/usr/bin/env python3
"""Real HTTP/PostgreSQL coverage for schedule media-version preconditions.

Uses a disposable database and media directory and never contacts providers.
Build the server and set CARGO_TARGET_DIR to its build directory before running.
"""
import base64
import concurrent.futures
import http.cookiejar
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid

sys.dont_write_bytecode = True
import schedule_requests as common

ROOT, CONTAINER, USER, PORT, DB, TARGET = (common.ROOT, common.CONTAINER, common.USER,
                                          common.PORT, common.DB, common.TARGET)
sql = common.sql
stamp = common.timestamp


def run():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    origin = f"http://127.0.0.1:{port}"
    process = None
    locker = None
    with tempfile.TemporaryDirectory(prefix="beweft-schedule-media-") as media_dir:
        env = dict(os.environ, DATABASE_URL=f"postgres://{USER}:{common.PASSWORD}@localhost:{PORT}/{DB}",
                   BEWEFT_PUBLIC_URL=origin, BEWEFT_BIND=f"127.0.0.1:{port}", BEWEFT_MEDIA_DIR=media_dir,
                   BEWEFT_SETUP_SECRET="isolated-media-schedule-secret", BEWEFT_ENABLE_FIXTURES="true")
        process = subprocess.Popen([str(TARGET / "debug/beweft-server")], cwd=ROOT, env=env,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

        def send(path, body=None, method=None, token=None):
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
                    return result.status, json.load(result)
            except urllib.error.HTTPError as error:
                return error.code, json.load(error)

        def call(path, body=None, method=None, expected=200, token=None):
            status, value = send(path, body, method, token)
            assert status == expected, (path, status, expected, value)
            return value["data"] if status == 200 else value

        def upload(brand, name):
            png = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=")
            boundary = "beweft-media-versions"
            body = (f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="{name}.png"\r\nContent-Type: image/png\r\n\r\n'.encode()
                    + png + f"\r\n--{boundary}--\r\n".encode())
            request = urllib.request.Request(origin + f"/api/v1/media?brand_id={brand}", data=body,
                headers={"X-Beweft-Request":"1", "Origin":origin,
                         "Content-Type":f"multipart/form-data; boundary={boundary}"})
            with client.open(request, timeout=10) as result:
                assert result.status == 200
                return json.load(result)["data"]

        def media_conflict(post, channels, versions):
            request_id = str(uuid.uuid4())
            body = {"version": post["version"], "channel_ids": channels, "scheduled_at": stamp(),
                    "request_id": request_id, "media_versions": versions}
            result = call(f"/posts/{post['id']}/schedule?view=summary", body, expected=409)
            assert result["error"]["code"] == "media_version_conflict", result
            call(f"/schedule-requests/{request_id}", expected=404)
            assert sql(f"SELECT count(*) FROM publications WHERE post_id='{post['id']}'") == "0"

        def lock(query):
            proc = subprocess.Popen(["docker", "exec", "-i", CONTAINER, "psql", "-X", "-qAt", "-U", USER, "-d", DB,
                                     "-v", "ON_ERROR_STOP=1"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            proc.stdin.write(f"BEGIN; {query}; SELECT 'held';\n")
            proc.stdin.flush()
            while proc.stdout.readline().strip() != "held":
                assert proc.poll() is None, "fixture lock failed"
            return proc

        def release(proc):
            proc.stdin.write("COMMIT;\n\\q\n")
            proc.stdin.flush()
            proc.wait(timeout=5)

        def wait_query(fragment):
            for _ in range(100):
                if sql(f"SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%{fragment}%')") == "t":
                    return
                time.sleep(0.05)
            raise AssertionError(f"database query did not block: {fragment}")

        try:
            for _ in range(100):
                try:
                    if send("/auth/status")[0] == 200:
                        break
                except OSError:
                    assert process.poll() is None, "server exited during startup"
                    time.sleep(0.1)
            else:
                raise AssertionError("server did not start")
            # Applying the new migration to a populated precondition-free
            # request table leaves existing rows with a NULL precondition.
            migration = (ROOT / "migrations/0011_schedule_media_versions.sql").read_text()
            upgrade = sql("BEGIN; CREATE TEMP TABLE schedule_requests(id integer PRIMARY KEY); "
                          "INSERT INTO schedule_requests(id) VALUES(1); " + migration +
                          " SELECT media_versions IS NULL FROM schedule_requests WHERE id=1; ROLLBACK;")
            assert upgrade == "t"
            call("/auth/bootstrap", {"setup_secret": env["BEWEFT_SETUP_SECRET"],
                                      "email":"media@example.invalid", "password":"Isolated-media-password!"})
            brand = call("/brands", {"name":"Media scheduling"})["id"]
            other_brand = call("/brands", {"name":"Other media"})["id"]
            channels = [call("/channels", {"brand_id":brand, "name":f"Channel {i}", "provider":"manual"})["id"] for i in range(5)]
            media = [upload(brand, f"fixture-{i}") for i in range(4)]
            other_media = upload(other_brand, "other")
            a, b, c, d = (item["id"] for item in media)
            assert all(item["metadata_version"] == 1 for item in media)
            post = call("/posts", {"brand_id":brand, "text":"Variant media", "media_ids":[a,b], "variants":[
                {"channel_id":channels[0], "media_ids":[c]},
                {"channel_id":channels[2], "media_ids":[]},
                {"channel_id":channels[3], "media_ids":[d]},
            ]})
            key = str(uuid.uuid4())
            selected = [channels[0], channels[1], channels[2]]
            versions = {c.upper():1, b:1, a:1}
            body = {"version":1, "channel_ids":selected, "scheduled_at":stamp(), "request_id":key, "media_versions":versions}
            path = f"/posts/{post['id']}/schedule"
            initial = call(path, body)
            receipt = call(f"/schedule-requests/{key}")
            canonical = {item:1 for item in sorted([a,b,c])}
            assert receipt["request"]["media_versions"] == canonical
            assert list(receipt["request"]["media_versions"]) == sorted(canonical)
            by_channel = {row["channel_id"]:row for row in initial}
            assert by_channel[channels[0]]["payload"]["media_ids"] == [c]
            assert by_channel[channels[1]]["payload"]["media_ids"] == [a,b]
            assert by_channel[channels[2]]["payload"]["media_ids"] == []
            assert d not in canonical
            replay = call(path + "?view=summary", {**body, "channel_ids":selected[::-1], "media_versions":canonical})
            assert [row["id"] for row in replay] == [row["id"] for row in receipt["publications"]]

            # The old raw-JSON API stored UUID spellings as supplied. Reads
            # must let current clients select those variants, and scheduling
            # must use the same UUID-value selection for admission and payload.
            historical = call("/posts", {"brand_id":brand, "text":"Historical variants", "media_ids":[a,d]})
            raw_media = [a.replace("-", "").upper(), "{" + d.upper() + "}"]
            raw_variants = [
                {"channel_id":channels[0].upper(), "text":"Uppercase variant", "media_ids":[b.upper()],
                 "options":{"original_spelling":b.upper()}, "legacy_extra":{"id":b.upper()}},
                {"channel_id":"{" + channels[1].upper() + "}", "media_ids":["{" + c.upper() + "}"]},
                {"channel_id":channels[2].replace("-", "").upper(), "text":"Empty override", "media_ids":[]},
            ]
            raw_options = {"original_spelling":a.upper(), "nested":{"id":d.upper()}}
            sql(f"UPDATE posts SET media_ids='{json.dumps(raw_media)}'::jsonb, "
                f"variants='{json.dumps(raw_variants)}'::jsonb, options='{json.dumps(raw_options)}'::jsonb "
                f"WHERE id='{historical['id']}'")
            raw_select = ("SELECT jsonb_build_object('media_ids',media_ids,'variants',variants,'options',options)::text "
                          f"FROM posts WHERE id='{historical['id']}'")
            raw_before = json.loads(sql(raw_select))
            expected_variants = [
                {**raw_variants[0], "channel_id":channels[0], "media_ids":[b]},
                {**raw_variants[1], "channel_id":channels[1], "media_ids":[c]},
                {**raw_variants[2], "channel_id":channels[2]},
            ]
            def assert_historical(value, summary=False):
                assert value["media_ids"] == [a,d], value
                if not summary:
                    assert value["variants"] == expected_variants, value
                    assert value["options"] == raw_options, value

            loaded = call(f"/posts/{historical['id']}")
            assert_historical(loaded)
            for suffix, summary in [("", False), ("&limit=10", False),
                                    ("&view=summary", True), ("&limit=10&view=summary", True)]:
                rows = call(f"/posts?brand_id={brand}{suffix}")
                assert_historical(next(row for row in rows if row["id"] == historical["id"]), summary)
            assert json.loads(sql(raw_select)) == raw_before
            # An unrelated edit also preserves raw fields while its response
            # shows the normalized representation.
            historical = call(f"/posts/{historical['id']}", {"version":1, "title":"Historical title"}, "PATCH")
            assert historical["version"] == 2
            assert_historical(historical)
            assert json.loads(sql(raw_select)) == raw_before

            # Model the client selecting variants from the normalized response,
            # including an explicitly empty media override.
            chosen = [channels[0], channels[1], channels[2]]
            effective = set()
            for channel in chosen:
                variant = next((item for item in loaded["variants"] if item["channel_id"] == channel), None)
                effective.update(variant["media_ids"] if variant is not None and "media_ids" in variant else loaded["media_ids"])
            assert effective == {b,c}
            media_conflict(historical, chosen, {a:1,d:1})
            historical_key = str(uuid.uuid4())
            historical_body = {"version":historical["version"], "channel_ids":chosen, "scheduled_at":stamp(),
                               "request_id":historical_key, "media_versions":{item:1 for item in effective}}
            historical_rows = call(f"/posts/{historical['id']}/schedule", historical_body)
            historical_by_channel = {item["channel_id"]:item for item in historical_rows}
            assert historical_by_channel[channels[0]]["payload"]["media_ids"] == [b]
            assert historical_by_channel[channels[0]]["payload"]["text"] == "Uppercase variant"
            assert historical_by_channel[channels[0]]["payload"]["options"] == raw_variants[0]["options"]
            assert historical_by_channel[channels[1]]["payload"]["media_ids"] == [c]
            assert historical_by_channel[channels[2]]["payload"]["media_ids"] == []
            assert call(f"/schedule-requests/{historical_key}")["request"]["media_versions"] == {b:1,c:1}

            # A historical frozen publication retains its original spelling;
            # replacing it uses the selected variant and canonical IDs anew.
            original = historical_by_channel[channels[0]]
            sql(f"UPDATE publications SET payload=jsonb_set(payload,'{{media_ids}}','[\"{b.upper()}\"]'::jsonb) "
                f"WHERE id='{original['id']}'")
            replacement = call(f"/publications/{original['id']}/replace", {
                "post_version":historical["version"], "publication_post_version":historical["version"],
                "scheduled_at":original["scheduled_at"]})["publication"]
            assert replacement["payload"]["media_ids"] == [b]
            replayed = call(f"/posts/{historical['id']}/schedule", historical_body)
            replayed_by_channel = {item["channel_id"]:item for item in replayed}
            assert replayed_by_channel[channels[0]]["id"] == original["id"]
            assert replayed_by_channel[channels[0]]["payload"]["media_ids"] == [b.upper()]
            assert replayed_by_channel[channels[0]]["state"] == "canceled"
            assert {item["id"] for item in call(f"/schedule-requests/{historical_key}")["publications"]} == {item["id"] for item in historical_rows}
            assert json.loads(sql(raw_select)) == raw_before

            # Receipt identity uses the original map; later media edits must not
            # change its snapshot or turn a same-key replay into a new conflict.
            updated = call(f"/media/{a}", {"alt_text":"After committed schedule", "metadata_version":1}, "PATCH")
            assert updated["metadata_version"] == 2
            replay = call(path, {**body, "media_versions":canonical})
            assert [row["id"] for row in replay] == [row["id"] for row in initial]
            old_meta = next(row for row in replay if row["channel_id"] == channels[1])["payload"]["media_metadata"]
            assert next(row for row in old_meta if row["id"] == a)["alt_text"] == ""
            assert call(f"/schedule-requests/{key}")["request"]["media_versions"] == canonical
            for changed in [{**canonical, a:2}, {a:1,b:1}, {**canonical,d:1}, None]:
                # A previous rejected attempt can briefly retain the request
                # gate while its transaction rolls back. The documented busy
                # result is inconclusive; only test mismatch after admission.
                deadline = time.monotonic() + 5
                while True:
                    result = call(path, {**body, "media_versions":changed}, expected=409)
                    if result["error"]["code"] != "schedule_request_in_progress":
                        break
                    assert time.monotonic() < deadline, result
                    time.sleep(0.01)
                assert result["error"]["code"] == "schedule_request_conflict", result

            # Discard a committed response, edit metadata, then recover and
            # retry the original immutable map without resnapshotting.
            lost_post = call("/posts", {"brand_id":brand, "text":"Lost media response", "media_ids":[d]})
            lost_key = str(uuid.uuid4())
            lost_body = {"version":1, "channel_ids":[channels[4]], "scheduled_at":stamp(), "request_id":lost_key,
                         "media_versions":{d:1}}
            lost_request = urllib.request.Request(origin + f"/api/v1/posts/{lost_post['id']}/schedule?view=summary",
                data=json.dumps(lost_body).encode(), headers={"X-Beweft-Request":"1", "Origin":origin, "Content-Type":"application/json"})
            with client.open(lost_request, timeout=10) as committed:
                assert committed.status == 200  # Deliberately do not read its body.
            edited = call(f"/media/{d}", {"alt_text":"Edited after lost response", "metadata_version":1}, "PATCH")
            assert edited["metadata_version"] == 2
            lost_receipt = call(f"/schedule-requests/{lost_key}")
            assert lost_receipt["request"]["media_versions"] == {d:1}
            recovered = call(f"/posts/{lost_post['id']}/schedule", lost_body)
            assert recovered[0]["id"] == lost_receipt["publications"][0]["id"]
            assert recovered[0]["payload"]["media_metadata"] == [{"id":d, "alt_text":""}]
            changed = call(f"/posts/{lost_post['id']}/schedule", {**lost_body, "media_versions":{d:2}}, expected=409)
            assert changed["error"]["code"] == "schedule_request_conflict"

            # A preflight followed by an external edit is a definite conflict
            # only after this new key has been admitted; no request is stored.
            preflight = call(f"/media/{a}")
            updated = call(f"/media/{a}", {"alt_text":"Changed between preflight and commit", "metadata_version":preflight["metadata_version"]}, "PATCH")
            stale_post = call("/posts", {"brand_id":brand, "text":"Stale preflight", "media_ids":[a]})
            media_conflict(stale_post, [channels[4]], {a:preflight["metadata_version"]})
            fresh_key = str(uuid.uuid4())
            fresh_body = {"version":1, "channel_ids":[channels[4]], "scheduled_at":stamp(), "request_id":fresh_key,
                          "media_versions":{a:updated["metadata_version"]}}
            assert len(call(f"/posts/{stale_post['id']}/schedule?view=summary", fresh_body)) == 1

            # Missing, extra, invalid and foreign-scope map entries are rejected
            # before any publication or receipt can commit.
            invalid_post = call("/posts", {"brand_id":brand, "text":"Invalid sets", "media_ids":[a,b], "variants":[
                {"channel_id":channels[0], "media_ids":[c]}, {"channel_id":channels[2], "media_ids":[]},
                {"channel_id":channels[3], "media_ids":[d]},
            ]})
            current = {a:updated["metadata_version"], b:1, c:1}
            for invalid in [{a:updated["metadata_version"],b:1}, {**current,d:1}, {**current,other_media["id"]:1},
                            {**current,a:0}, {**current,a:-1}, {**current,a:9223372036854775807},
                            {str(uuid.uuid4()):1 for _ in range(1001)}]:
                media_conflict(invalid_post, selected, invalid)
            duplicate_id = next(item for item in [a,b,c] if item.upper() != item)
            duplicate = {**current, duplicate_id.upper():current[duplicate_id]}
            duplicate_key = str(uuid.uuid4())
            duplicate_body = {"version":1, "channel_ids":selected, "scheduled_at":stamp(), "request_id":duplicate_key, "media_versions":duplicate}
            call(f"/posts/{invalid_post['id']}/schedule", duplicate_body, expected=422)
            call(f"/schedule-requests/{duplicate_key}", expected=404)

            # Even an existing asset outside this post's brand is unavailable;
            # this fixture corruption does not reveal the other brand's data.
            sql(f"UPDATE media SET brand_id='{other_brand}' WHERE id='{a}'")
            media_conflict(invalid_post, selected, current)
            sql(f"UPDATE media SET brand_id='{brand}' WHERE id='{a}'")

            # Legacy omission stays usable and stores null. Explicit empty is
            # a separate immutable intent for text-only schedules.
            legacy_media = next(item for item in [a,b,c,d] if item.upper() != item)
            legacy_version = call(f"/media/{legacy_media}")["metadata_version"]
            legacy = call("/posts", {"brand_id":brand, "text":"Legacy", "media_ids":[legacy_media]})
            # Legacy media spellings are canonical in new snapshots even when
            # the request omits the metadata-version precondition.
            sql(f"UPDATE posts SET media_ids='[\"{legacy_media.upper()}\"]'::jsonb WHERE id='{legacy['id']}'")
            legacy_key = str(uuid.uuid4())
            legacy_body = {"version":1, "channel_ids":[channels[4]], "scheduled_at":stamp(), "request_id":legacy_key}
            legacy_rows = call(f"/posts/{legacy['id']}/schedule", legacy_body)
            assert legacy_rows[0]["payload"]["media_ids"] == [legacy_media]
            assert call(f"/schedule-requests/{legacy_key}")["request"]["media_versions"] is None
            call(f"/media/{legacy_media}", {"alt_text":"Legacy changed later", "metadata_version":legacy_version}, "PATCH")
            assert call(f"/posts/{legacy['id']}/schedule", legacy_body)[0]["id"] == legacy_rows[0]["id"]
            empty = call("/posts", {"brand_id":brand, "text":"Empty map"})
            empty_key = str(uuid.uuid4())
            empty_body = {"version":1, "channel_ids":[channels[4]], "scheduled_at":stamp(), "request_id":empty_key, "media_versions":{}}
            call(f"/posts/{empty['id']}/schedule?view=summary", empty_body)
            assert call(f"/schedule-requests/{empty_key}")["request"]["media_versions"] == {}
            result = call(f"/posts/{empty['id']}/schedule", {**empty_body, "media_versions":None}, expected=409)
            assert result["error"]["code"] == "schedule_request_conflict"

            # Hold the last media row exclusively. Scheduling must lock the
            # first media row before waiting on the last one, and do so before
            # publication writes. A concurrent metadata edit of the first row
            # waits until the schedule commits its old snapshot.
            pair = sorted([a,b])
            before = {item:call(f"/media/{item}") for item in pair}
            locked_post = call("/posts", {"brand_id":brand, "text":"Ordered media locks", "media_ids":pair[::-1]})
            locked_key = str(uuid.uuid4())
            locked_body = {"version":1, "channel_ids":[channels[4]], "scheduled_at":stamp(), "request_id":locked_key,
                           "media_versions":{item:before[item]["metadata_version"] for item in pair[::-1]}}
            locker = lock(f"SELECT id FROM media WHERE id='{pair[1]}' FOR UPDATE")
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                scheduling = pool.submit(call, f"/posts/{locked_post['id']}/schedule", locked_body)
                wait_query("ORDER BY id FOR SHARE")
                call(f"/schedule-requests/{locked_key}", expected=404)
                in_progress = call(f"/posts/{locked_post['id']}/schedule", {**locked_body, "media_versions":{}}, expected=409)
                assert in_progress["error"]["code"] == "schedule_request_in_progress"
                editing = pool.submit(call, f"/media/{pair[0]}",
                    {"alt_text":"After ordered schedule", "metadata_version":before[pair[0]]["metadata_version"]}, "PATCH")
                wait_query("UPDATE media SET alt_text")
                assert not scheduling.done() and not editing.done()
                assert sql(f"SELECT count(*) FROM publications WHERE post_id='{locked_post['id']}'") == "0"
                release(locker)
                locker = None
                committed = scheduling.result(timeout=10)
                edited = editing.result(timeout=10)
            assert edited["metadata_version"] == before[pair[0]]["metadata_version"] + 1
            metadata = committed[0]["payload"]["media_metadata"]
            assert next(row for row in metadata if row["id"] == pair[0])["alt_text"] == before[pair[0]]["alt_text"]
            assert call(f"/schedule-requests/{locked_key}")["request"]["media_versions"] == locked_body["media_versions"]
            assert call(f"/posts/{locked_post['id']}/schedule", locked_body)[0]["id"] == committed[0]["id"]

            # While channel admission is blocked, media is not locked. An edit
            # can win; the stale schedule must then reject before any snapshot.
            before_channel = call(f"/media/{pair[1]}")
            channel_post = call("/posts", {"brand_id":brand, "text":"Channel lock race", "media_ids":[pair[1]]})
            channel_key = str(uuid.uuid4())
            channel_body = {"version":1, "channel_ids":[channels[4]], "scheduled_at":stamp(), "request_id":channel_key,
                            "media_versions":{pair[1]:before_channel["metadata_version"]}}
            locker = lock(f"SELECT pg_advisory_xact_lock(hashtextextended('{channels[4]}',0))")
            with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
                scheduling = pool.submit(call, f"/posts/{channel_post['id']}/schedule", channel_body, expected=409)
                wait_query("pg_advisory_xact_lock(hashtextextended")
                call(f"/schedule-requests/{channel_key}", expected=404)
                started = time.monotonic()
                edited = call(f"/media/{pair[1]}",
                    {"alt_text":"Edit won channel admission", "metadata_version":before_channel["metadata_version"]}, "PATCH")
                assert time.monotonic() - started < 2, "channel wait held a media lock"
                assert edited["metadata_version"] == before_channel["metadata_version"] + 1
                release(locker)
                locker = None
                rejection = scheduling.result(timeout=10)
            assert rejection["error"]["code"] == "media_version_conflict"
            call(f"/schedule-requests/{channel_key}", expected=404)
            assert sql(f"SELECT count(*) FROM publications WHERE post_id='{channel_post['id']}'") == "0"

            # After channel and media admission, pause at the first publication
            # insert. A later edit waits for the committed original snapshot.
            before_insert = call(f"/media/{pair[1]}")
            insert_post = call("/posts", {"brand_id":brand, "text":"Precommit edit race", "media_ids":[pair[1]]})
            insert_key = str(uuid.uuid4())
            insert_body = {"version":1, "channel_ids":[channels[4]], "scheduled_at":stamp(), "request_id":insert_key,
                           "media_versions":{pair[1]:before_insert["metadata_version"]}}
            sql("CREATE FUNCTION fixture_wait_publication() RETURNS trigger LANGUAGE plpgsql AS $$ "
                f"BEGIN IF NEW.post_id='{insert_post['id']}' THEN PERFORM pg_advisory_xact_lock(hashtextextended('fixture-publication-gate',0)); END IF; "
                "RETURN NEW; END $$; CREATE TRIGGER fixture_wait_publication BEFORE INSERT ON publications "
                "FOR EACH ROW EXECUTE FUNCTION fixture_wait_publication();")
            locker = lock("SELECT pg_advisory_xact_lock(hashtextextended('fixture-publication-gate',0))")
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                scheduling = pool.submit(call, f"/posts/{insert_post['id']}/schedule", insert_body)
                wait_query("INSERT INTO publications")
                call(f"/schedule-requests/{insert_key}", expected=404)
                editing = pool.submit(call, f"/media/{pair[1]}",
                    {"alt_text":"After committed snapshot", "metadata_version":before_insert["metadata_version"]}, "PATCH")
                wait_query("UPDATE media SET alt_text")
                assert not scheduling.done() and not editing.done()
                assert sql(f"SELECT count(*) FROM publications WHERE post_id='{insert_post['id']}'") == "0"
                release(locker)
                locker = None
                committed = scheduling.result(timeout=10)
                edited = editing.result(timeout=10)
            sql("DROP TRIGGER fixture_wait_publication ON publications; DROP FUNCTION fixture_wait_publication();")
            assert edited["metadata_version"] == before_insert["metadata_version"] + 1
            assert committed[0]["payload"]["media_metadata"] == [{"id":pair[1], "alt_text":before_insert["alt_text"]}]
            assert call(f"/posts/{insert_post['id']}/schedule", insert_body)[0]["id"] == committed[0]["id"]
            assert call(f"/schedule-requests/{insert_key}")["request"]["media_versions"] == insert_body["media_versions"]

            # A brand-restricted token cannot inspect another brand's receipt.
            token = call("/tokens", {"name":"Other brand", "scopes":["read"], "brand_ids":[other_brand]})["token"]
            denied = call(f"/schedule-requests/{locked_key}", expected=404, token=token)
            missing = call(f"/schedule-requests/{uuid.uuid4()}", expected=404, token=token)
            assert denied == missing
            print("PASS media preconditions, canonical variant union, snapshot ordering, edits, replay, scope and legacy recovery")
        finally:
            if locker is not None:
                locker.stdin.write("ROLLBACK;\n\\q\n")
                locker.stdin.flush()
                locker.wait(timeout=5)
            if process is not None:
                process.terminate()
                process.wait(timeout=10)


if __name__ == "__main__":
    created = False
    try:
        subprocess.run(["docker", "exec", CONTAINER, "createdb", "-U", USER, DB], check=True)
        created = True
        run()
    finally:
        if created:
            subprocess.run(["docker", "exec", CONTAINER, "dropdb", "-U", USER, "--force", DB], check=True)
