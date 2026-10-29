#!/usr/bin/env python3
"""Production router and worker against an isolated loopback provider and database.

Build the server/worker with --features provider-fixtures first. This test uses no
real provider accounts, public posts, or paid APIs, and removes all test resources.
"""
import datetime as dt
import http.server
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
sys.dont_write_bytecode = True
from fixture_test_support import enter_loopback_sandbox, loopback_only

ROOT = Path(__file__).resolve().parents[3]
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target"))).resolve()
CONTAINER = os.environ.get("BEWEFT_TEST_POSTGRES_CONTAINER", "beweft-dev-postgres")
USER = os.environ.get("BEWEFT_TEST_DB_USER", "beweft")
PASSWORD = os.environ.get("BEWEFT_TEST_DB_PASSWORD", "beweft-local-development")
PORT = os.environ.get("BEWEFT_TEST_DB_PORT", "55439")
DB = "beweft_router_provider_" + uuid.uuid4().hex[:12]


def sql(query):
    return subprocess.run(["docker", "exec", CONTAINER, "psql", "-X", "-qAt", "-U", USER,
                           "-d", DB, "-v", "ON_ERROR_STOP=1", "-c", query],
                          check=True, capture_output=True, text=True).stdout.strip()


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


class Fixture(http.server.BaseHTTPRequestHandler):
    app_origin = ""
    lock = threading.Lock()
    counts = {}
    photos = []
    feed_mode = "success"
    reply_mode = "success"
    reply_started = threading.Event()
    reply_release = threading.Event()
    reply_returned = threading.Event()
    feed_started = threading.Event()
    feed_release = threading.Event()
    feed_returned = threading.Event()
    x_media_block = False
    x_media_started = threading.Event()
    x_media_release = threading.Event()
    x_media_returned = threading.Event()

    def log_message(self, *_):
        pass

    def record(self, key):
        with self.lock:
            self.counts[key] = self.counts.get(key, 0) + 1

    def respond(self, value, status=200, headers=None):
        payload = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        for name, value in (headers or {}).items():
            self.send_header(name, value)
        self.end_headers()
        try:
            self.wfile.write(payload)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def do_GET(self):
        path = urllib.parse.urlsplit(self.path).path
        self.record("GET " + path)
        if path == "/v23.0/oauth/access_token":
            return self.respond({"access_token":"facebook-long-fixture", "expires_in":7200,
                                 "scope":"pages_manage_posts pages_read_engagement pages_manage_engagement"})
        if path == "/v23.0/me/accounts":
            return self.respond({"data":[{"id":"destination", "name":"Fixture Page",
                                           "access_token":"page-fixture", "tasks":["CREATE_CONTENT"]}]})
        if path == "/2/users/me":
            return self.respond({"data":{"id":"x-destination", "name":"Fixture X"}})
        if path == "/youtube/v3/channels":
            return self.respond({"items":[{"id":"youtube-destination", "snippet":{"title":"Fixture YouTube"}}]})
        if path == "/v23.0/photo-post":
            return self.respond({"reactions":{"summary":{"total_count":3}},
                                 "comments":{"summary":{"total_count":1}}})
        if path == "/v23.0/photo-post/comments":
            return self.respond({"data":[{"id":"comment-1", "message":"Fixture comment",
                                           "from":{"name":"Fixture viewer"}}]})
        return self.respond({}, 404)

    def do_POST(self):
        path = urllib.parse.urlsplit(self.path).path
        body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        self.record("POST " + path)
        if path == "/v23.0/oauth/access_token":
            return self.respond({"access_token":"facebook-short-fixture", "expires_in":7200})
        if path == "/token":
            return self.respond({"access_token":"youtube-fixture", "refresh_token":"youtube-refresh-fixture", "expires_in":7200,
                                 "scope":"https://www.googleapis.com/auth/youtube.upload https://www.googleapis.com/auth/youtube.readonly https://www.googleapis.com/auth/youtube.force-ssl"})
        if path == "/2/oauth2/token":
            form = urllib.parse.parse_qs(body.decode())
            refresh = form.get("grant_type") == ["refresh_token"]
            if refresh:
                self.record("x_refresh")
            return self.respond({"access_token":"x-refresh-fixture" if refresh else "x-original-fixture",
                                 "refresh_token":"x-next-fixture", "expires_in":7200 if refresh else -1200,
                                 "scope":"tweet.read tweet.write users.read offline.access media.write"})
        if path == "/2/tweets":
            if self.headers.get("Authorization") == "Bearer x-refresh-fixture":
                self.record("x_rotated_authorization")
            return self.respond({"data":{"id":"x-post"}})
        if path == "/2/media/upload":
            if self.x_media_block:
                self.x_media_started.set()
                self.x_media_release.wait(20)
            self.respond({"data":{"id":"x-media-fixture"}})
            self.x_media_returned.set()
            return
        if path == "/upload/youtube/v3/videos":
            return self.respond({}, headers={"Location":"https://www.googleapis.com/upload/youtube/v3/videos/session-fixture"})
        if path == "/v23.0/destination/photos":
            value = json.loads(body)
            url = value["url"].replace("https://media.fixture", self.app_origin, 1)
            try:
                with urllib.request.urlopen(url, timeout=5) as response:
                    data = response.read()
                    assert response.status == 200
            except Exception:
                return self.respond({}, 502)
            with self.lock:
                self.photos.append(data)
            return self.respond({"id":"photo", "post_id":"photo-post"})
        if path == "/v23.0/destination/feed":
            mode = self.feed_mode
            if mode == "uncertain":
                return self.respond({}, 500)
            if mode == "blocked":
                self.feed_started.set()
                self.feed_release.wait(20)
            self.respond({"id":"feed-post"})
            if mode == "blocked":
                self.feed_returned.set()
            return
        if path == "/v23.0/comment-1/comments":
            if self.reply_mode == "uncertain":
                return self.respond({}, 500)
            if self.reply_mode == "blocked":
                self.reply_started.set()
                self.reply_release.wait(20)
                self.respond({"id":"reply-1"})
                self.reply_returned.set()
                return
            return self.respond({"id":"reply-1"})
        return self.respond({}, 404)

    def do_PUT(self):
        path = urllib.parse.urlsplit(self.path).path
        self.rfile.read(int(self.headers.get("Content-Length", "0")))
        self.record("PUT " + path)
        if path == "/upload/youtube/v3/videos/session-fixture":
            return self.respond({}, 404)
        return self.respond({}, 404)


def run():
    app_origin = f"http://127.0.0.1:{free_port()}"
    fixture = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    Fixture.app_origin = app_origin
    fixture_origin = f"http://127.0.0.1:{fixture.server_address[1]}"
    fixture_thread = threading.Thread(target=fixture.serve_forever, daemon=True)
    fixture_thread.start()
    processes = []
    locker = None
    opener = urllib.request.build_opener(NoRedirect())
    cookies = {}
    with tempfile.TemporaryDirectory(prefix="beweft-provider-router-") as temp:
        # Shift only the worker's wall clock, not the database, host or test
        # process. Confirm that Rust's SystemTime actually sees the offset.
        shim = Path(temp) / "fixture-clock-skew.dylib"
        probe_source = Path(temp) / "clock-probe.rs"
        probe_binary = Path(temp) / "clock-probe"
        monotonic_source = Path(temp) / "monotonic-probe.c"
        monotonic_binary = Path(temp) / "monotonic-probe"
        subprocess.run(["cc", "-dynamiclib", ROOT / "crates/server/tests/fixture_clock_skew.c", "-o", shim], check=True)
        probe_source.write_text('fn main(){println!("{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs())}')
        subprocess.run(["rustc", probe_source, "-o", probe_binary], check=True)
        monotonic_source.write_text('#include <time.h>\n#include <stdio.h>\nint main(void){struct timespec t; clock_gettime(CLOCK_MONOTONIC,&t); printf("%lld.%09ld",(long long)t.tv_sec,t.tv_nsec);}')
        subprocess.run(["cc", monotonic_source, "-o", monotonic_binary], check=True)
        skew_env = dict(os.environ, DYLD_INSERT_LIBRARIES=str(shim), BEWEFT_TEST_CLOCK_OFFSET_SECONDS="-600")
        skewed_time = int(subprocess.check_output([probe_binary], env=skew_env))
        assert 595 <= time.time() - skewed_time <= 605, "worker clock skew was not applied"
        normal_mono = float(subprocess.check_output([monotonic_binary]))
        skewed_mono = float(subprocess.check_output([monotonic_binary], env=skew_env))
        assert 0 <= skewed_mono - normal_mono < 3, "fixture changed the monotonic clock"
        env = dict(os.environ, DATABASE_URL=f"postgres://{USER}:{PASSWORD}@localhost:{PORT}/{DB}",
                   BEWEFT_PUBLIC_URL="https://media.fixture", BEWEFT_BIND=app_origin.removeprefix("http://"),
                   BEWEFT_MEDIA_DIR=temp, BEWEFT_SETUP_SECRET="isolated-provider-router-secret",
                   BEWEFT_ENCRYPTION_KEY="41" * 32, BEWEFT_ENABLE_FIXTURES="true",
                   BEWEFT_REQUIRE_PROVIDER_FIXTURES="true", BEWEFT_PROVIDER_FIXTURE_ORIGIN=fixture_origin)
        server = subprocess.Popen(loopback_only([TARGET / "debug/beweft-server"]), cwd=ROOT, env=env,
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        processes.append(server)

        def request(path, body=None, method=None, token=None, cookie=True, expected=200, api_origin=None):
            headers = {"Origin":"https://media.fixture", "X-Beweft-Request":"1"}
            if cookie and cookies:
                headers["Cookie"] = "; ".join(f"{key}={value}" for key, value in cookies.items())
            if token is not None:
                headers["Authorization"] = "Bearer " + token
            if body is not None:
                headers["Content-Type"] = "application/json"
            request = urllib.request.Request((api_origin or app_origin) + "/api/v1" + path,
                data=None if body is None else json.dumps(body).encode(), headers=headers,
                method=method or ("POST" if body is not None else "GET"))
            try:
                response = opener.open(request, timeout=15)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                status = response.status
                for value in response.headers.get_all("Set-Cookie", []):
                    name, contents = value.split(";", 1)[0].split("=", 1)
                    cookies[name] = contents
                raw = response.read()
                value = json.loads(raw) if raw else None
            assert status == expected, (path, status, expected, value)
            return value.get("data") if status == 200 else value

        def upload(brand, data, filename="fixture.jpg", mime="image/jpeg"):
            boundary = "provider-router-fixture"
            body = (f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="{filename}"\r\nContent-Type: {mime}\r\n\r\n'.encode()
                    + data + f"\r\n--{boundary}--\r\n".encode())
            headers = {"Origin":"https://media.fixture", "X-Beweft-Request":"1",
                       "Cookie":"; ".join(f"{k}={v}" for k,v in cookies.items()),
                       "Content-Type":f"multipart/form-data; boundary={boundary}"}
            req = urllib.request.Request(app_origin + f"/api/v1/media?brand_id={brand}", data=body, headers=headers)
            with opener.open(req, timeout=15) as response:
                return json.load(response)["data"]

        def wait_for(predicate, description, seconds=20):
            deadline = time.monotonic() + seconds
            while time.monotonic() < deadline:
                value = predicate()
                if value:
                    return value
                time.sleep(0.1)
            raise AssertionError("Timed out: " + description)

        def connect(provider, brand, reconnect=None):
            started = request(f"/connections/{provider}/start", {"brand_id":brand, "reconnect_channel_id":reconnect})
            state = urllib.parse.parse_qs(urllib.parse.urlsplit(started["authorization_url"]).query)["state"][0]
            request(f"/connections/{provider}/callback?" + urllib.parse.urlencode({"state":state, "code":"fixture-code"}), expected=303)
            flow = request(f"/connection-flows/{started['flow_id']}")
            destination = flow["destinations"][0]["id"]
            return request(f"/connection-flows/{started['flow_id']}/complete", {"destination_id":destination})

        def publication(pub_id):
            return request(f"/publications?id={pub_id}&limit=1")[0]

        def schedule(brand, channel, text, media=(), title="", options=None, delay=1):
            post = request("/posts", {"brand_id":brand, "title":title, "text":text,
                                      "media_ids":list(media), "options":options or {}})
            stamp = (dt.datetime.now(dt.timezone.utc) + dt.timedelta(seconds=delay)).isoformat(timespec="microseconds")
            return request(f"/posts/{post['id']}/schedule", {"version":1,"channel_ids":[channel],
                           "scheduled_at":stamp,"request_id":str(uuid.uuid4())})[0]

        def state_is(pub_id, state):
            value = publication(pub_id)
            return value if value["state"] == state else None

        def ready():
            if server.poll() is not None:
                raise AssertionError("server exited during startup")
            try:
                return request("/auth/status", cookie=False)
            except OSError:
                return False

        try:
            wait_for(ready, "server startup")
            request("/auth/bootstrap", {"setup_secret":env["BEWEFT_SETUP_SECRET"],
                                       "email":"router@example.invalid", "password":"Fixture-password-long!"})
            # Migration preserves older accepted receipts and keeps its
            # terminal rejection constraint non-null, using temporary tables.
            migration = (ROOT / "migrations/0012_schedule_rejections.sql").read_text()
            assert sql("BEGIN; CREATE TEMP TABLE posts(id uuid PRIMARY KEY); "
                       "CREATE TEMP TABLE schedule_requests(id uuid,post_id uuid REFERENCES posts(id)); "
                       "INSERT INTO schedule_requests(id) VALUES(gen_random_uuid()); " + migration +
                       "SELECT outcome='accepted' AND error_code IS NULL AND error_message IS NULL FROM schedule_requests; ROLLBACK;") == "t"
            # Even a fixture-capable test build does not expose fixture mode
            # unless the explicit runtime opt-in is present.
            disabled_env = dict(env)
            disabled_env.pop("BEWEFT_ENABLE_FIXTURES")
            # A configured fixture origin must never fall through to a live
            # provider when its runtime opt-in is missing.
            rejected = subprocess.Popen(loopback_only([TARGET / "debug/beweft-server"]), cwd=ROOT, env=disabled_env,
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            assert rejected.wait(timeout=5) != 0
            disabled_env.pop("BEWEFT_PROVIDER_FIXTURE_ORIGIN")
            disabled_env.pop("BEWEFT_REQUIRE_PROVIDER_FIXTURES")
            disabled_port = free_port()
            disabled_env["BEWEFT_BIND"] = f"127.0.0.1:{disabled_port}"
            disabled = subprocess.Popen(loopback_only([TARGET / "debug/beweft-server"]), cwd=ROOT, env=disabled_env,
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            processes.append(disabled)
            def disabled_catalog():
                if disabled.poll() is not None:
                    raise AssertionError("fixture-disabled server exited")
                req = urllib.request.Request(f"http://127.0.0.1:{disabled_port}/api/v1/providers",
                    headers={"Cookie":f"beweft_session={cookies['beweft_session']}"})
                try:
                    with opener.open(req, timeout=1) as response:
                        return json.load(response)["data"]
                except OSError:
                    return False
            assert not any(item["id"] == "fixture" for item in wait_for(disabled_catalog, "fixture-disabled catalog"))
            disabled.send_signal(signal.SIGTERM)
            disabled.wait(timeout=10)
            request("/provider-apps", cookie=False, expected=401)
            assert request("/connections/youtube/removal-status")["revocation_id"] is None
            assert sql("SELECT count(*) FROM provider_oauth_fences WHERE provider='youtube'") == "0"
            request("/connections/facebook/callback?state=unknown&code=unknown", cookie=False, expected=400)
            request(f"/provider-media/{uuid.uuid4()}?token={'a' * 64}", cookie=False, expected=404)
            providers = request("/providers")
            facebook = next(item for item in providers if item["id"] == "facebook")
            assert facebook["implementation"] == "available" and facebook["configuration"] == "not_configured"
            assert facebook["verification"] == "unverified_live" and facebook["inbox"]
            brand = request("/brands", {"name":"Router fixture"})["id"]
            other_brand = request("/brands", {"name":"Other fixture"})["id"]
            request("/provider-apps/facebook", {"client_id":"facebook-client", "client_secret":"fixture-secret"}, "PUT")
            request("/provider-apps/x", {"client_id":"x-client", "client_secret":"fixture-secret"}, "PUT")
            request("/provider-apps/youtube", {"client_id":"youtube-client", "client_secret":"fixture-secret"}, "PUT")
            apps = request("/provider-apps")
            assert all("client_secret" not in item and "revocation_id" in item for item in apps)
            assert next(item for item in request("/providers") if item["id"] == "facebook")["configuration"] == "configured"
            fb = connect("facebook", brand)
            sibling = connect("facebook", other_brand)
            x = connect("x", brand)
            youtube = connect("youtube", brand)
            assert fb["connection_status"] == "connected" and fb["manual_guidance"] is None
            assert fb["capabilities"]["verification"] == "identity_verified_publishing_unverified"
            assert fb["capabilities"]["permissions"]["actions"]["publish"]["status"] == "granted"
            token = request("/tokens", {"name":"Read only", "scopes":["read"], "brand_ids":[brand]})["token"]
            request("/provider-apps", token=token, expected=403)
            request("/connections/facebook/removal-status", token=token, expected=403)
            request(f"/channels/{fb['id']}/disconnect", {"confirm_shared_impact":True}, token=token, expected=403)
            request(f"/channels/{fb['id']}/sync", {}, token=token, expected=403)
            request("/analytics", token="invalid-token", expected=401)
            request(f"/analytics?brand_id={other_brand}", token=token, expected=403)

            # New scheduling validates exactly the adapter's local rules and
            # media bytes before committing any receipt or publication.
            image = Path(temp) / "fixture.jpg"
            subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-f", "lavfi", "-i", "color=c=blue:s=640x480:r=10",
                            "-frames:v", "1", str(image)], check=True)
            image_bytes = image.read_bytes()
            asset = upload(brand, image_bytes)
            video = Path(temp) / "fixture.mp4"
            subprocess.run(["ffmpeg", "-nostdin", "-v", "error", "-f", "lavfi", "-i", "color=c=blue:s=640x480:r=10",
                            "-t", "4", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-movflags", "+faststart", str(video)], check=True)
            video_asset = upload(brand, video.read_bytes(), "fixture.mp4", "video/mp4")
            corrupt = upload(brand, b"\xff\xd8\xffinvalid-image", "corrupt.jpg")
            before_validation = dict(Fixture.counts)
            when = (dt.datetime.now(dt.timezone.utc) + dt.timedelta(hours=1)).isoformat(timespec="microseconds")
            manual = request("/channels", {"brand_id":brand,"name":"Atomic manual","provider":"manual"})["id"]
            def rejected_schedule(post, channels):
                request_id = str(uuid.uuid4())
                intent = {"version":1,"channel_ids":channels,"scheduled_at":when,"request_id":request_id}
                result = request(f"/posts/{post['id']}/schedule", intent, expected=400)
                assert result["error"]["code"] == "provider_format_invalid", result
                assert sql(f"SELECT count(*) FROM publications WHERE post_id='{post['id']}'") == "0"
                receipt = request(f"/schedule-requests/{request_id}")
                assert receipt["outcome"] == "rejected" and receipt["error"] == result["error"]
                assert receipt["request"]["channel_ids"] == sorted(channels) and receipt["publications"] == []
                return intent, result
            too_long = request("/posts", {"brand_id":brand,"text":"x" * 281})
            rejected_intent, rejected_result = rejected_schedule(too_long, [manual, x["id"]])
            # Treat the original 400 as lost. A changed adapter rule cannot
            # make an in-flight retry of the same key create publications.
            sql(f"UPDATE channels SET provider='manual' WHERE id='{x['id']}'")
            replay = request(f"/posts/{too_long['id']}/schedule", rejected_intent, expected=400)
            assert replay == rejected_result
            assert request(f"/posts/{too_long['id']}/schedule", {**rejected_intent,"version":2}, expected=409)["error"]["code"] == "schedule_request_conflict"
            fresh_intent = {**rejected_intent,"request_id":str(uuid.uuid4())}
            corrected = request(f"/posts/{too_long['id']}/schedule", fresh_intent)
            assert len(corrected) == 2
            for item in corrected:
                request(f"/publications/{item['id']}/cancel", {})
            sql(f"UPDATE channels SET provider='x' WHERE id='{x['id']}'")
            assert request(f"/schedule-requests/{rejected_intent['request_id']}")["publications"] == []
            variant_invalid = request("/posts", {"brand_id":brand,"text":"base is valid",
                                      "variants":[{"channel_id":x["id"],"text":"x" * 281}]})
            deleted_intent, deleted_result = rejected_schedule(variant_invalid, [manual, x["id"]])
            other_token = request("/tokens", {"name":"Other brand scope", "scopes":["publishing:write"],
                                             "brand_ids":[other_brand]})["token"]
            request(f"/posts/{variant_invalid['id']}", method="DELETE")
            request(f"/posts/{variant_invalid['id']}", expected=404)
            assert request(f"/schedule-requests/{deleted_intent['request_id']}")["outcome"] == "rejected"
            assert request(f"/posts/{variant_invalid['id']}/schedule", deleted_intent, expected=400) == deleted_result
            request(f"/schedule-requests/{deleted_intent['request_id']}", token=other_token, expected=404)
            request(f"/posts/{variant_invalid['id']}/schedule", deleted_intent, token=other_token, expected=404)
            unsupported = request("/posts", {"brand_id":brand,"text":"video","media_ids":[video_asset["id"]]})
            rejected_schedule(unsupported, [x["id"]])
            malformed = request("/posts", {"brand_id":brand,"text":"bad image","media_ids":[corrupt["id"]]})
            rejected_schedule(malformed, [x["id"]])
            empty = request("/posts", {"brand_id":brand,"text":""})
            rejected_schedule(empty, [x["id"]])
            legacy_empty = request("/posts", {"brand_id":brand,"text":""})
            legacy_id = str(uuid.uuid4())
            legacy_body = {"version":1,"channel_ids":[manual],"scheduled_at":when,"request_id":legacy_id}
            assert request(f"/posts/{legacy_empty['id']}/schedule", legacy_body, expected=400)["error"]["code"] == "invalid_request"
            request(f"/schedule-requests/{legacy_id}", expected=404)

            # Local file disappearance or read failure is infrastructure,
            # not a permanent format decision. The exact same intent can be
            # retried after the original immutable media is available again.
            media_key = sql(f"SELECT storage_key FROM media WHERE id='{asset['id']}'")
            media_path = Path(temp) / media_key
            held_path = Path(temp) / "temporarily-held-image"
            for inaccessible in ("missing", "unreadable"):
                transient = request("/posts", {"brand_id":brand,"text":"retry image","media_ids":[asset["id"]]})
                transient_id = str(uuid.uuid4())
                transient_body = {"version":1,"channel_ids":[x["id"]],"scheduled_at":when,"request_id":transient_id}
                if inaccessible == "missing":
                    media_path.rename(held_path)
                else:
                    media_path.chmod(0)
                try:
                    result = request(f"/posts/{transient['id']}/schedule", transient_body, expected=503)
                    assert result["error"]["code"] == "provider_format_check_unavailable"
                    request(f"/schedule-requests/{transient_id}", expected=404)
                    assert sql(f"SELECT count(*) FROM publications WHERE post_id='{transient['id']}'") == "0"
                finally:
                    if inaccessible == "missing":
                        held_path.rename(media_path)
                    else:
                        media_path.chmod(0o600)
                accepted_transient = request(f"/posts/{transient['id']}/schedule", transient_body)[0]
                request(f"/publications/{accepted_transient['id']}/cancel", {})
            missing_option = request("/posts", {"brand_id":brand,"title":"Video","text":"video",
                                      "media_ids":[video_asset["id"]],"options":{"privacy":"private"}})
            rejected_schedule(missing_option, [youtube["id"]])
            mastodon = str(uuid.uuid4())
            sql(f"INSERT INTO channels(id,brand_id,name,provider,paused) VALUES('{mastodon}','{brand}','Static options','mastodon',true)")
            invalid_option = request("/posts", {"brand_id":brand,"text":"visibility", "options":{"visibility":"invalid"}})
            rejected_schedule(invalid_option, [mastodon])

            valid = request("/posts", {"brand_id":brand,"text":"valid image","media_ids":[asset["id"]]})
            intent = {"version":1,"channel_ids":[x["id"]],"scheduled_at":when,"request_id":str(uuid.uuid4())}
            accepted = request(f"/posts/{valid['id']}/schedule?view=summary", intent)[0]
            # Replay remains authoritative after the installed capability has
            # changed, even though a new LinkedIn media intent would fail.
            sql(f"UPDATE channels SET provider='linkedin' WHERE id='{x['id']}'")
            assert request(f"/posts/{valid['id']}/schedule?view=summary", intent)[0]["id"] == accepted["id"]
            receipt = request(f"/schedule-requests/{intent['request_id']}")
            assert receipt["outcome"] == "accepted" and receipt["error"] is None
            assert receipt["publications"][0]["id"] == accepted["id"]
            sql(f"UPDATE channels SET provider='x' WHERE id='{x['id']}'")
            changed = request(f"/posts/{valid['id']}", {"version":1,"text":"x" * 281}, "PATCH")
            replacement = request(f"/publications/{accepted['id']}/replace",
                                  {"post_version":changed["version"],"publication_post_version":1,
                                   "scheduled_at":accepted["scheduled_at"]}, expected=400)
            assert replacement["error"]["code"] == "provider_format_invalid"
            assert publication(accepted["id"])["state"] == "queued"
            assert sql(f"SELECT count(*) FROM publications WHERE post_id='{valid['id']}'") == "1"
            request(f"/publications/{accepted['id']}/cancel", {})
            assert Fixture.counts == before_validation, "scheduling validation must make zero provider calls"

            # X metered reads are refused without an explicit acknowledgement.
            before_x_reads = Fixture.counts.get("GET /2/tweets/x-post", 0)
            request(f"/channels/{x['id']}/sync", {}, expected=400)
            assert Fixture.counts.get("GET /2/tweets/x-post", 0) == before_x_reads

            # Paused channels do not claim; once resumed, a real worker performs
            # refresh, provider POST and an attempt-fenced receipt commit.
            request(f"/channels/{x['id']}", {"paused":True}, "PATCH")
            x_post = schedule(brand, x["id"], "Fixture X text")
            sql("CREATE TABLE fixture_lease_deltas (publication_id uuid, remaining double precision); "
                "CREATE FUNCTION fixture_capture_lease() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN "
                "INSERT INTO fixture_lease_deltas SELECT NEW.publication_id,EXTRACT(EPOCH FROM (lease_until-clock_timestamp())) "
                "FROM publications WHERE id=NEW.publication_id; RETURN NEW; END $$; "
                "CREATE TRIGGER fixture_capture_lease AFTER INSERT ON publication_attempts FOR EACH ROW EXECUTE FUNCTION fixture_capture_lease();")
            worker_env = dict(env, DYLD_INSERT_LIBRARIES=str(shim), BEWEFT_TEST_CLOCK_OFFSET_SECONDS="-600")
            worker = subprocess.Popen(loopback_only([TARGET / "debug/beweft-worker"]), cwd=ROOT, env=worker_env,
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            processes.append(worker)
            time.sleep(3)
            assert publication(x_post["id"])["state"] == "queued"
            assert Fixture.counts.get("POST /2/tweets", 0) == 0
            request(f"/channels/{x['id']}", {"paused":False}, "PATCH")
            published = wait_for(lambda: state_is(x_post["id"], "published"), "X worker receipt")
            assert sql(f"SELECT remaining BETWEEN 295 AND 300 FROM fixture_lease_deltas WHERE publication_id='{x_post['id']}'") == "t"
            assert published["external_id"] == "x-post" and Fixture.counts.get("x_refresh", 0) == 1
            assert Fixture.counts.get("x_rotated_authorization", 0) == 1
            attempt = request(f"/publications/{x_post['id']}/attempts")[0]
            assert attempt["state"] == "published" and attempt["receipt"]["verification"] == "unverified_live"

            # Expired and sub-second leases cannot start even the refresh call.
            # The short lease is set by PostgreSQL during claim, so this test
            # does not adjust either the shared host or database clock.
            short_x = connect("x", other_brand)
            expired = schedule(other_brand, short_x["id"], "Expired admission", delay=60)
            near = schedule(other_brand, short_x["id"], "Near expiry admission", delay=61)
            sql("CREATE FUNCTION fixture_admission_lease() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN "
                f"IF NEW.publication_id='{expired['id']}' THEN UPDATE publications SET lease_until=clock_timestamp()-interval '1 second' WHERE id=NEW.publication_id; "
                f"ELSIF NEW.publication_id='{near['id']}' THEN UPDATE publications SET lease_until=clock_timestamp()+interval '0.5 seconds' WHERE id=NEW.publication_id; "
                "END IF; RETURN NEW; END $$; CREATE TRIGGER fixture_admission_lease BEFORE INSERT ON publication_attempts "
                "FOR EACH ROW EXECUTE FUNCTION fixture_admission_lease();")
            before_short = dict(Fixture.counts)
            sql(f"UPDATE publications SET scheduled_at=clock_timestamp() WHERE id='{expired['id']}'")
            wait_for(lambda: state_is(expired["id"], "uncertain"), "expired lease admission")
            sql(f"UPDATE publications SET scheduled_at=clock_timestamp() WHERE id='{near['id']}'")
            wait_for(lambda: state_is(near["id"], "uncertain"), "near-expiry lease admission")
            assert Fixture.counts == before_short, "an expired or near-expiry claim must make zero provider calls"
            assert sql(f"SELECT count(*) FROM publication_attempts WHERE publication_id IN ('{expired['id']}','{near['id']}') AND (receipt ? 'provider_receipt' OR receipt->>'verification'='unverified_live')") == "0"
            sql("DROP TRIGGER fixture_admission_lease ON publication_attempts; DROP FUNCTION fixture_admission_lease();")

            # A stalled channel-row lock must not hold an expired claim or
            # begin its refresh after the database-derived deadline.
            blocked = schedule(other_brand, short_x["id"], "Blocked channel admission", delay=60)
            sql("CREATE FUNCTION fixture_blocked_lease() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN "
                f"IF NEW.id='{blocked['id']}' AND NEW.state='sending' THEN NEW.lease_until=clock_timestamp()+interval '3 seconds'; END IF; "
                "RETURN NEW; END $$; CREATE TRIGGER fixture_blocked_lease BEFORE UPDATE OF state ON publications "
                "FOR EACH ROW EXECUTE FUNCTION fixture_blocked_lease();")
            locker = subprocess.Popen(["docker", "exec", "-i", CONTAINER, "psql", "-X", "-qAt", "-U", USER, "-d", DB,
                                       "-v", "ON_ERROR_STOP=1"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            # NO KEY UPDATE blocks the worker's row lock without blocking the
            # foreign-key checks needed to commit the initial attempt.
            locker.stdin.write(f"BEGIN; SELECT id FROM channels WHERE id='{short_x['id']}' FOR NO KEY UPDATE; SELECT 'held';\n")
            locker.stdin.flush()
            while locker.stdout.readline().strip() != "held":
                assert locker.poll() is None, "channel row lock fixture failed"
            before_blocked = dict(Fixture.counts)
            sql(f"UPDATE publications SET scheduled_at=clock_timestamp() WHERE id='{blocked['id']}'")
            try:
                wait_for(lambda: state_is(blocked["id"], "uncertain"), "bounded channel-row wait", seconds=12)
            except AssertionError:
                diagnostic = sql(f"SELECT state||':'||COALESCE((lease_until>clock_timestamp())::text,'none')||':'||(SELECT count(*) FROM publication_attempts WHERE publication_id='{blocked['id']}') FROM publications WHERE id='{blocked['id']}'")
                waits = sql("SELECT COALESCE(string_agg(COALESCE(wait_event_type,'none')||':'||COALESCE(wait_event,'none'),','),'none') FROM pg_stat_activity WHERE datname=current_database() AND pid<>pg_backend_pid() AND state<>'idle'")
                raise AssertionError(f"bounded channel-row wait failed; publication={diagnostic}, database waits={waits}") from None
            assert Fixture.counts == before_blocked, "a blocked expired stage must make zero provider calls"
            locker.stdin.write("COMMIT;\n\\q\n")
            locker.stdin.flush()
            locker.wait(timeout=5)
            locker = None
            sql("DROP TRIGGER fixture_blocked_lease ON publications; DROP FUNCTION fixture_blocked_lease();")

            # A refresh can rotate a token before local storage passes the
            # lease boundary. Persist that rotation, but do not send the post.
            refresh_late = schedule(other_brand, short_x["id"], "Refresh crosses lease", delay=60)
            sql("CREATE FUNCTION fixture_refresh_lease() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN "
                f"IF NEW.publication_id='{refresh_late['id']}' THEN UPDATE publications SET lease_until=clock_timestamp()+interval '3 seconds' WHERE id=NEW.publication_id; END IF; "
                "RETURN NEW; END $$; CREATE TRIGGER fixture_refresh_lease BEFORE INSERT ON publication_attempts "
                "FOR EACH ROW EXECUTE FUNCTION fixture_refresh_lease(); "
                "CREATE FUNCTION fixture_slow_rotation() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN "
                f"IF NEW.id='{short_x['id']}' AND NEW.encrypted_credentials IS DISTINCT FROM OLD.encrypted_credentials THEN PERFORM pg_sleep(4); END IF; "
                "RETURN NEW; END $$; CREATE TRIGGER fixture_slow_rotation BEFORE UPDATE OF encrypted_credentials ON channels "
                "FOR EACH ROW EXECUTE FUNCTION fixture_slow_rotation();")
            refresh_before = Fixture.counts.get("x_refresh", 0)
            posts_before = Fixture.counts.get("POST /2/tweets", 0)
            sql(f"UPDATE publications SET scheduled_at=clock_timestamp() WHERE id='{refresh_late['id']}'")
            wait_for(lambda: state_is(refresh_late["id"], "uncertain"), "refresh crosses lease", seconds=25)
            assert Fixture.counts.get("x_refresh", 0) == refresh_before + 1
            assert Fixture.counts.get("POST /2/tweets", 0) == posts_before
            assert sql(f"SELECT count(*) FROM publication_attempts WHERE publication_id='{refresh_late['id']}' AND receipt IS NOT NULL") == "0"
            sql("DROP TRIGGER fixture_refresh_lease ON publication_attempts; DROP FUNCTION fixture_refresh_lease(); "
                "DROP TRIGGER fixture_slow_rotation ON channels; DROP FUNCTION fixture_slow_rotation();")
            rotated = schedule(other_brand, short_x["id"], "Use persisted rotation")
            wait_for(lambda: state_is(rotated["id"], "published"), "persisted refresh token")
            assert Fixture.counts.get("x_refresh", 0) == refresh_before + 1
            assert Fixture.counts.get("x_rotated_authorization", 0) == 2

            # The provider fetches the committed media capability while its
            # publish POST is in progress, through the actual public router.
            photo = schedule(brand, fb["id"], "Fixture photo", [asset["id"]])
            published = wait_for(lambda: state_is(photo["id"], "published"), "Facebook media receipt")
            assert published["external_id"] == "photo-post" and Fixture.photos == [image_bytes]
            assert request(f"/publications/{photo['id']}/attempts")[0]["receipt"]["provider_receipt"]["id"] == "photo-post"
            assert request(f"/publications/{photo['id']}/provider-status")["session_expired"] is False

            # Suspend the actual worker while an accepted media stage returns
            # after its lease. It must not persist progress, IDs, or receipts.
            before_expiring_tweets = Fixture.counts.get("POST /2/tweets", 0)
            expiring = schedule(brand, x["id"], "Expiring media stage", [asset["id"]], delay=60)
            sql("CREATE FUNCTION fixture_short_lease() RETURNS trigger LANGUAGE plpgsql AS $$ "
                f"BEGIN IF NEW.publication_id='{expiring['id']}' THEN UPDATE publications SET lease_until=clock_timestamp()+interval '4 seconds' WHERE id=NEW.publication_id; END IF; "
                "RETURN NEW; END $$; CREATE TRIGGER fixture_short_lease BEFORE INSERT ON publication_attempts "
                "FOR EACH ROW EXECUTE FUNCTION fixture_short_lease();")
            Fixture.x_media_block = True
            sql(f"UPDATE publications SET scheduled_at=clock_timestamp() WHERE id='{expiring['id']}'")
            assert Fixture.x_media_started.wait(20), "X media stage did not start"
            worker.send_signal(signal.SIGSTOP)
            Fixture.x_media_release.set()
            assert Fixture.x_media_returned.wait(5), "X media response did not return"
            time.sleep(5)
            worker.send_signal(signal.SIGCONT)
            wait_for(lambda: state_is(expiring["id"], "uncertain"), "expired network stage quarantine")
            assert sql(f"SELECT count(*) FROM provider_delivery_checkpoints WHERE publication_id='{expiring['id']}'") == "0"
            assert sql(f"SELECT COALESCE(external_id,'none') FROM publications WHERE id='{expiring['id']}'") == "none"
            assert sql(f"SELECT count(*) FROM publication_attempts WHERE publication_id='{expiring['id']}' AND receipt IS NOT NULL") == "0"
            assert Fixture.counts.get("POST /2/tweets", 0) == before_expiring_tweets
            Fixture.x_media_block = False
            sql("DROP TRIGGER fixture_short_lease ON publication_attempts; DROP FUNCTION fixture_short_lease();")

            # A real worker saves a resumable YouTube checkpoint; a 404 probe
            # makes it expired but never proves that the video was unpublished.
            upload_post = schedule(brand, youtube["id"], "Fixture video", [video_asset["id"]],
                                   title="Fixture title", options={"privacy":"private","made_for_kids":False})
            wait_for(lambda: state_is(upload_post["id"], "uncertain"), "uncertain YouTube upload")
            assert sql(f"SELECT count(*) FROM provider_delivery_checkpoints WHERE publication_id='{upload_post['id']}'") == "1"
            probe = request(f"/publications/{upload_post['id']}/provider-reconcile", {})
            assert probe["state"] == "uncertain" and probe["session_expired"] is True
            assert request(f"/publications/{upload_post['id']}/provider-status")["session_expired"] is True
            request(f"/publications/{upload_post['id']}/reset-expired-session", {}, expected=409)
            request(f"/publications/{upload_post['id']}/reconcile", {"resolution":"not_published","note":"Fixture inspected"})
            assert request(f"/publications/{upload_post['id']}/provider-status")["reset_allowed"] is True
            assert request(f"/publications/{upload_post['id']}/reset-expired-session", {})["session_reset"] is True
            assert sql(f"SELECT count(*) FROM provider_delivery_checkpoints WHERE publication_id='{upload_post['id']}'") == "0"
            assert publication(upload_post["id"])["state"] == "failed"

            # Actual sync, observation reads, reply idempotency and owner resolution.
            synced = request(f"/channels/{fb['id']}/sync", {})
            assert synced["metrics_observed"] == 2 and synced["inbox_items"] == 1
            analytics = request(f"/analytics?brand_id={brand}", token=token)
            assert analytics["availability"] == "partial" and analytics["metrics"][0]["publication_id"] == photo["id"]
            item = request(f"/inbox?brand_id={brand}", token=token)["items"][0]
            request(f"/inbox/{item['id']}/read", {})
            reply_key = str(uuid.uuid4())
            reply = {"request_id":reply_key,"text":"Fixture reply","expected_last_request_id":None}
            assert request(f"/inbox/{item['id']}/reply", reply)["state"] == "published"
            assert request(f"/inbox/{item['id']}/reply", reply)["id"] == reply_key
            assert Fixture.counts.get("POST /v23.0/comment-1/comments") == 1

            # Two additional isolated API processes see different wall clocks.
            # An ahead-of-DB host must still send a live reply; a behind-DB
            # host must not send an already near-expiry reply.
            def skewed_api(offset):
                api_port = free_port()
                api_origin = f"http://127.0.0.1:{api_port}"
                api_env = dict(env, BEWEFT_BIND=f"127.0.0.1:{api_port}",
                               DYLD_INSERT_LIBRARIES=str(shim), BEWEFT_TEST_CLOCK_OFFSET_SECONDS=str(offset))
                process = subprocess.Popen(loopback_only([TARGET / "debug/beweft-server"]), cwd=ROOT, env=api_env,
                                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                processes.append(process)
                def ready_skewed():
                    if process.poll() is not None:
                        raise AssertionError("clock-shifted API exited")
                    try:
                        return request("/auth/status", cookie=False, api_origin=api_origin)
                    except OSError:
                        return False
                wait_for(ready_skewed, "clock-shifted API")
                return process, api_origin

            ahead, ahead_origin = skewed_api(600)
            ahead_key = str(uuid.uuid4())
            before_ahead = Fixture.counts.get("POST /v23.0/comment-1/comments", 0)
            assert request(f"/inbox/{item['id']}/reply", {"request_id":ahead_key,"text":"Ahead clock fixture",
                           "expected_last_request_id":reply_key}, api_origin=ahead_origin)["state"] == "published"
            assert Fixture.counts.get("POST /v23.0/comment-1/comments", 0) == before_ahead + 1
            ahead.send_signal(signal.SIGTERM)
            ahead.wait(timeout=10)

            behind, behind_origin = skewed_api(-600)
            near_reply = str(uuid.uuid4())
            sql("CREATE FUNCTION fixture_short_reply() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN "
                f"IF NEW.id='{near_reply}' THEN NEW.delivery_deadline=clock_timestamp()+interval '0.5 seconds'; END IF; "
                "RETURN NEW; END $$; CREATE TRIGGER fixture_short_reply BEFORE INSERT ON provider_replies "
                "FOR EACH ROW EXECUTE FUNCTION fixture_short_reply();")
            before_near = Fixture.counts.get("POST /v23.0/comment-1/comments", 0)
            assert request(f"/inbox/{item['id']}/reply", {"request_id":near_reply,"text":"Near-expiry reply fixture",
                           "expected_last_request_id":ahead_key}, api_origin=behind_origin)["state"] == "uncertain"
            assert Fixture.counts.get("POST /v23.0/comment-1/comments", 0) == before_near
            wait_for(lambda: sql(f"SELECT delivery_deadline<=clock_timestamp() FROM provider_replies WHERE id='{near_reply}'") == "t", "reply deadline elapsed")
            request(f"/inbox/{item['id']}/replies/{near_reply}/resolve", {"resolution":"not_published","note":"Fixture saw no request"})
            sql("DROP TRIGGER fixture_short_reply ON provider_replies; DROP FUNCTION fixture_short_reply();")

            # If an accepted reply response returns after its DB deadline,
            # even a suspended API must not install the provider receipt.
            late_reply = str(uuid.uuid4())
            sql("CREATE FUNCTION fixture_late_reply() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN "
                f"IF NEW.id='{late_reply}' THEN NEW.delivery_deadline=clock_timestamp()+interval '3 seconds'; END IF; "
                "RETURN NEW; END $$; CREATE TRIGGER fixture_late_reply BEFORE INSERT ON provider_replies "
                "FOR EACH ROW EXECUTE FUNCTION fixture_late_reply();")
            Fixture.reply_mode = "blocked"
            late_result = []
            late_errors = []
            def late_reply_call():
                try:
                    late_result.append(request(f"/inbox/{item['id']}/reply", {"request_id":late_reply,"text":"Late reply fixture",
                                               "expected_last_request_id":near_reply}, api_origin=behind_origin))
                except Exception as error:
                    late_errors.append(error)
            late_thread = threading.Thread(target=late_reply_call)
            late_thread.start()
            assert Fixture.reply_started.wait(10), "reply did not reach the loopback fixture"
            behind.send_signal(signal.SIGSTOP)
            Fixture.reply_release.set()
            assert Fixture.reply_returned.wait(5), "fixture reply response was not released"
            time.sleep(4)
            behind.send_signal(signal.SIGCONT)
            late_thread.join(timeout=10)
            assert not late_thread.is_alive() and not late_errors and late_result[0]["state"] == "uncertain"
            assert sql(f"SELECT receipt IS NULL FROM provider_replies WHERE id='{late_reply}'") == "t"
            request(f"/inbox/{item['id']}/replies/{late_reply}/resolve", {"resolution":"published","note":"Fixture observed the reply"})
            sql("DROP TRIGGER fixture_late_reply ON provider_replies; DROP FUNCTION fixture_late_reply();")
            behind.send_signal(signal.SIGTERM)
            behind.wait(timeout=10)

            Fixture.reply_mode = "uncertain"
            uncertain_key = str(uuid.uuid4())
            assert request(f"/inbox/{item['id']}/reply", {"request_id":uncertain_key,"text":"Uncertain fixture reply",
                           "expected_last_request_id":late_reply})["state"] == "uncertain"
            assert request(f"/inbox/{item['id']}/replies")[0]["id"] == uncertain_key
            request(f"/inbox/{item['id']}/replies/{uncertain_key}/resolve", {"resolution":"not_published","note":"Fixture checked"}, token=token, expected=403)
            resolved = request(f"/inbox/{item['id']}/replies/{uncertain_key}/resolve", {"resolution":"not_published","note":"Fixture checked"})
            assert resolved["state"] == "failed" and resolved["receipt"]["verification"] == "owner_attestation"

            Fixture.feed_mode = "uncertain"
            uncertain = schedule(brand, fb["id"], "Uncertain fixture post")
            wait_for(lambda: state_is(uncertain["id"], "uncertain"), "uncertain provider write")
            writes = Fixture.counts["POST /v23.0/destination/feed"]
            request(f"/publications/{uncertain['id']}/provider-reconcile", {}, expected=400)
            assert Fixture.counts["POST /v23.0/destination/feed"] == writes
            request(f"/publications/{uncertain['id']}/reconcile", {"resolution":"not_published","note":"Fixture checked"})
            assert publication(uncertain["id"])["state"] == "failed"

            # A late provider response from a replaced attempt cannot write
            # external IDs, receipts or state into the new sending attempt.
            Fixture.feed_mode = "blocked"
            stale = schedule(brand, fb["id"], "Blocked stale fixture post")
            assert Fixture.feed_started.wait(20), "provider POST did not start"
            old_attempt = sql(f"SELECT id FROM publication_attempts WHERE publication_id='{stale['id']}' AND state='sending'")
            terminated = sql("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname=current_database() "
                             "AND state='idle in transaction' AND query LIKE '%provider_delivery_checkpoints%' AND pid<>pg_backend_pid()")
            assert terminated == "t", terminated
            new_attempt = str(uuid.uuid4())
            sql(f"UPDATE publication_attempts SET state='uncertain',finished_at=now() WHERE id='{old_attempt}'; "
                f"UPDATE publications SET state='sending',lease_until=now()+interval '5 minutes' WHERE id='{stale['id']}'; "
                f"INSERT INTO publication_attempts(id,publication_id,state,provider) VALUES('{new_attempt}','{stale['id']}','sending','facebook')")
            Fixture.feed_release.set()
            assert Fixture.feed_returned.wait(5), "late provider response was not released"
            wait_for(lambda: sql(f"SELECT state FROM publication_attempts WHERE id='{old_attempt}'") == "uncertain", "old attempt fenced")
            time.sleep(2)
            assert sql(f"SELECT state||':'||COALESCE(external_id,'none')||':'||COALESCE(external_url,'none') FROM publications WHERE id='{stale['id']}'") == "sending:none:none"
            assert sql(f"SELECT state||':'||(receipt IS NULL)::text FROM publication_attempts WHERE id='{new_attempt}'") == "sending:true"
            assert sql(f"SELECT state||':'||(receipt IS NULL)::text FROM publication_attempts WHERE id='{old_attempt}'") == "uncertain:true"
            sql(f"UPDATE publication_attempts SET state='uncertain',finished_at=now() WHERE id='{new_attempt}'; "
                f"UPDATE publications SET state='uncertain',lease_until=NULL WHERE id='{stale['id']}'")

            # Unconfirmed disconnect has no effect. Two distinct pending
            # boundaries prove a delayed acknowledgement cannot clear the next.
            request(f"/channels/{fb['id']}", {"paused":True}, "PATCH")
            queued = schedule(brand, fb["id"], "Canceled by disconnect")
            request(f"/channels/{sibling['id']}", {"paused":True}, "PATCH")
            sibling_queued = schedule(other_brand, sibling["id"], "Sibling retained")
            request(f"/brands/{other_brand}", {"archived":True}, "PATCH")
            time.sleep(3)
            assert publication(sibling_queued["id"])["state"] == "queued"
            request(f"/brands/{other_brand}", {"archived":False}, "PATCH")
            fence_before = sql("SELECT oauth_generation||':'||COALESCE(revocation_id::text,'none') FROM provider_oauth_fences WHERE provider='facebook'")
            for invalid in [{}, {"confirm_shared_impact":False}]:
                result = request(f"/channels/{fb['id']}/disconnect", invalid, expected=400)
                assert result["error"]["code"] == "disconnect_confirmation_required"
            assert publication(queued["id"])["state"] == "queued"
            assert sql("SELECT oauth_generation||':'||COALESCE(revocation_id::text,'none') FROM provider_oauth_fences WHERE provider='facebook'") == fence_before
            assert next(item for item in request(f"/channels?brand_id={other_brand}") if item["id"] == sibling["id"])["connection_status"] == "connected"
            first = request(f"/channels/{fb['id']}/disconnect", {"confirm_shared_impact":True})
            first_id = first["revocation_id"]
            assert first_id and first["provider_revoked"] is False and first["channels_requiring_reconnection"] >= 1
            assert publication(queued["id"])["state"] == "canceled"
            assert publication(sibling_queued["id"])["state"] == "queued"
            assert next(item for item in request("/provider-apps") if item["provider"] == "facebook")["revocation_id"] == first_id
            assert request("/connections/facebook/removal-status")["revocation_id"] == first_id
            locker = subprocess.Popen(["docker", "exec", "-i", CONTAINER, "psql", "-X", "-qAt", "-U", USER, "-d", DB,
                                       "-v", "ON_ERROR_STOP=1"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            locker.stdin.write("BEGIN; SELECT pg_advisory_xact_lock(hashtextextended('provider-operation:facebook',0)); SELECT 'held';\n")
            locker.stdin.flush()
            while locker.stdout.readline().strip() != "held":
                assert locker.poll() is None, "provider status lock fixture failed"
            busy = request("/connections/facebook/removal-status", expected=409)
            assert "retry and check the current provider status" in busy["error"]["message"]
            assert "First inspect your channels" not in busy["error"]["message"]
            locker.stdin.write("COMMIT;\n\\q\n")
            locker.stdin.flush()
            locker.wait(timeout=5)
            locker = None
            request("/connections/facebook/acknowledge-revocation", {"access_removed":True,"revocation_id":first_id})
            sibling = connect("facebook", other_brand, sibling["id"])
            second = request(f"/channels/{sibling['id']}/disconnect", {"confirm_shared_impact":True})
            second_id = second["revocation_id"]
            assert second_id and second_id != first_id
            result = request("/connections/facebook/acknowledge-revocation", {"access_removed":True,"revocation_id":first_id}, expected=409)
            assert result["error"]["code"] == "revocation_id_conflict"
            assert next(item for item in request("/provider-apps") if item["provider"] == "facebook")["revocation_id"] == second_id
            assert request("/connections/facebook/removal-status")["revocation_id"] == second_id
            request("/connections/facebook/acknowledge-revocation", {"access_removed":True,"revocation_id":second_id})

            # A possible grant without saved app configuration still has a
            # recoverable removal ID if the disconnect response was lost.
            sql(f"DELETE FROM provider_apps WHERE provider='facebook'; UPDATE channels SET connection_status='reconnect_required' WHERE id='{fb['id']}'")
            orphan = request(f"/channels/{fb['id']}/disconnect", {"confirm_shared_impact":True})["revocation_id"]
            assert orphan and not any(item["provider"] == "facebook" for item in request("/provider-apps"))
            assert request("/connections/facebook/removal-status")["revocation_id"] == orphan
            request("/connections/facebook/acknowledge-revocation", {"access_removed":True,"revocation_id":orphan})
            assert not any("revoke" in key for key in Fixture.counts), Fixture.counts
            print("PASS production router, OAuth, worker receipts, media capability, observations, replies, recovery and revocation fences")
        finally:
            if locker is not None:
                locker.stdin.write("ROLLBACK;\n\\q\n")
                locker.stdin.flush()
                locker.wait(timeout=5)
            Fixture.feed_release.set()
            Fixture.x_media_release.set()
            Fixture.reply_release.set()
            for process in reversed(processes):
                if process.poll() is None:
                    process.send_signal(signal.SIGCONT)
                    process.send_signal(signal.SIGTERM)
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=5)
            fixture.shutdown()
            fixture.server_close()
            fixture_thread.join(timeout=5)


if __name__ == "__main__":
    enter_loopback_sandbox(allow_docker=True)
    created = False
    try:
        subprocess.run(["docker", "exec", CONTAINER, "createdb", "-U", USER, DB], check=True)
        created = True
        run()
    finally:
        if created:
            subprocess.run(["docker", "exec", CONTAINER, "dropdb", "-U", USER, "--force", DB], check=True)
