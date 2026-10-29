#!/usr/bin/env python3
"""Exercise the actual CLI and MCP subprocess against an isolated API/database.

Build first: cargo build -p beweft-server --bin beweft-server -p beweft-cli
Run: python3 crates/cli/tests/integration.py
Uses a uniquely named database in the configured test PostgreSQL container and
removes that database and its temporary files on exit. No worker or provider is run.
"""
import base64
import datetime as dt
import http.cookiejar
import http.server
import json
import os
from pathlib import Path
import select
import signal
import socket
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[3]
BIN = Path(os.environ.get("BEWEFT_TEST_BIN_DIR", ROOT / "target/debug"))
CONTAINER = os.environ.get("BEWEFT_TEST_POSTGRES_CONTAINER", "beweft-dev-postgres")
DB_USER = os.environ.get("BEWEFT_TEST_DB_USER", "beweft")
DB_PASSWORD = os.environ.get("BEWEFT_TEST_DB_PASSWORD", "beweft-local-development")
DB_PORT = os.environ.get("BEWEFT_TEST_DB_PORT", "55439")
DB = "beweft_cli_test_" + uuid.uuid4().hex[:12]


def unused_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def stop(process):
    if process.poll() is None:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)


def cli(env, *args, body=None, success=True):
    result = subprocess.run([str(BIN / "beweft"), *args], env=env,
                            input=None if body is None else json.dumps(body),
                            text=True, capture_output=True, timeout=20)
    assert (result.returncode == 0) == success, (args, result.returncode, result.stderr)
    assert env.get("BEWEFT_TOKEN", "unlikely-secret") not in result.stderr
    return json.loads(result.stdout) if success else result.stderr


class Mcp:
    def __init__(self, env):
        self.env = env
        self.p = subprocess.Popen([str(BIN / "beweft"), "mcp"], env=env,
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, bufsize=0)
        self.buffer = b""
        self.next_id = 1

    def send(self, message):
        data = message if isinstance(message, bytes) else json.dumps(message).encode()
        self.p.stdin.write(data + b"\n")
        self.p.stdin.flush()

    def receive(self):
        deadline = time.monotonic() + 15
        while b"\n" not in self.buffer:
            remaining = deadline - time.monotonic()
            assert remaining > 0 and select.select([self.p.stdout], [], [], remaining)[0], "MCP response timed out"
            data = os.read(self.p.stdout.fileno(), 65536)
            assert data, "MCP closed stdout unexpectedly"
            self.buffer += data
            assert len(self.buffer) <= 16 * 1024 * 1024
        line, self.buffer = self.buffer.split(b"\n", 1)
        return json.loads(line)

    def request(self, method, params=None):
        request_id = self.next_id
        self.next_id += 1
        message = {"jsonrpc": "2.0", "id": request_id, "method": method}
        if params is not None:
            message["params"] = params
        self.send(message)
        response = self.receive()
        assert response["jsonrpc"] == "2.0" and response["id"] == request_id, response
        return response

    def initialize(self, version="2025-11-25"):
        result = self.request("initialize", {"protocolVersion": version, "capabilities": {},
                                             "clientInfo": {"name": "isolated-test", "version": "1"}})["result"]
        assert result["protocolVersion"] == "2025-11-25" and "tools" in result["capabilities"]
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def call(self, name, args=None, error=False):
        result = self.request("tools/call", {"name": name, "arguments": args or {}})["result"]
        assert result.get("isError", False) == error, result
        assert self.env.get("BEWEFT_TOKEN", "unlikely-secret") not in json.dumps(result)
        if not error:
            assert json.loads(result["content"][0]["text"]) == result["structuredContent"]
            return result["structuredContent"]
        return result["content"][0]["text"]

    def close(self):
        if self.p.poll() is None:
            self.p.stdin.close()
            try:
                self.p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                stop(self.p)
        stderr = self.p.stderr.read().decode(errors="replace")
        assert self.env.get("BEWEFT_TOKEN", "unlikely-secret") not in stderr
        assert self.p.returncode == 0, stderr
        self.p.stdout.close()
        self.p.stderr.close()


def race_test(base_env, root, outside, brand):
    """Swap a file for a symlink after inspection but before upload starts."""
    inspected, resume = threading.Event(), threading.Event()
    received = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            assert self.path == "/api/v1/media/limits"
            if not inspected.is_set():
                inspected.set()
                assert resume.wait(10)
            payload = json.dumps({"data": {"max_upload_bytes": 1024 * 1024}}).encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        def do_POST(self):
            received.append(self.rfile.read(int(self.headers["Content-Length"])))
            payload = b'{"data":{"id":"fixture"}}'
            self.send_response(200)
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    env = dict(base_env, BEWEFT_URL=f"http://127.0.0.1:{server.server_port}", BEWEFT_UPLOAD_ROOT=str(root))
    mcp = Mcp(env)
    path = root / "race.png"
    safe_bytes = b"safe-open-file-content"
    secret_bytes = b"outside-secret-must-not-be-uploaded"
    path.write_bytes(safe_bytes)
    outside.write_bytes(secret_bytes)
    try:
        mcp.initialize()
        mcp.send({"jsonrpc": "2.0", "id": "race", "method": "tools/call", "params": {"name": "media_upload", "arguments": {"brand_id": brand, "path": str(path)}}})
        assert inspected.wait(10), "Upload did not inspect limits"
        path.rename(root / "held.png")
        path.symlink_to(outside)
        resume.set()
        response = mcp.receive()
        assert response["id"] == "race" and not response["result"]["isError"], response
        assert len(received) == 1 and safe_bytes in received[0] and secret_bytes not in received[0]
        # The root is opened when MCP starts; replacing its pathname later must
        # not redirect future uploads into the replacement directory either.
        (root / "root-swap.png").write_bytes(safe_bytes)
        replacement = root.parent / "replacement"
        replacement.mkdir()
        (replacement / "root-swap.png").write_bytes(secret_bytes)
        moved_root = root.parent / "held-root"
        root.rename(moved_root)
        root.symlink_to(replacement, target_is_directory=True)
        try:
            mcp.call("media_upload", {"brand_id": brand, "path": str(root / "root-swap.png")})
            assert len(received) == 2 and safe_bytes in received[1] and secret_bytes not in received[1]
        finally:
            root.unlink()
            moved_root.rename(root)
    finally:
        resume.set()
        mcp.close()
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def redirect_test(env):
    hits = []
    class Destination(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass
        def do_GET(self):
            hits.append(self.headers.get("Authorization"))
            self.send_response(200)
            self.end_headers()
    destination = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Destination)
    class Redirect(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass
        def do_GET(self):
            self.send_response(302)
            self.send_header("Location", f"http://127.0.0.1:{destination.server_port}/stolen")
            self.end_headers()
    redirect = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Redirect)
    threads = [threading.Thread(target=s.serve_forever, daemon=True) for s in (destination, redirect)]
    for thread in threads:
        thread.start()
    try:
        error = cli(dict(env, BEWEFT_URL=f"http://127.0.0.1:{redirect.server_port}"), "brands", success=False)
        assert "redirect" in error and not hits
    finally:
        for server in (redirect, destination):
            server.shutdown()
            server.server_close()
        for thread in threads:
            thread.join(timeout=5)


def lost_response_test(env, origin, brand):
    """Commit a write through the real API, then discard its response."""
    calls = []
    text = "lost-response-" + uuid.uuid4().hex
    class Proxy(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass
        def do_POST(self):
            calls.append(self.path)
            body = self.rfile.read(int(self.headers["Content-Length"]))
            request = urllib.request.Request(origin + self.path, data=body, headers={
                "Authorization": self.headers["Authorization"],
                "Content-Type": self.headers["Content-Type"],
                "X-Beweft-Request": "1",
            })
            with urllib.request.urlopen(request, timeout=10) as response:
                assert response.status == 200
                response.read()
            self.connection.shutdown(socket.SHUT_RDWR)
            self.connection.close()
    proxy = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Proxy)
    thread = threading.Thread(target=proxy.serve_forever, daemon=True)
    thread.start()
    try:
        error = cli(dict(env, BEWEFT_URL=f"http://127.0.0.1:{proxy.server_port}"), "draft-create", "--json", "-",
                    body={"body": {"brand_id": brand, "text": text}}, success=False)
        assert "outcome may be unknown" in error and calls == ["/api/v1/posts"]
        drafts = cli(env, "drafts")["data"]
        assert len([draft for draft in drafts if draft["text"] == text]) == 1
    finally:
        proxy.shutdown()
        proxy.server_close()
        thread.join(timeout=5)


def lost_upload_response_test(env, origin, brand, path):
    """Commit the multipart upload, then close before returning its response."""
    calls = []
    class Proxy(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass
        def do_GET(self):
            request = urllib.request.Request(origin + self.path, headers={"Authorization": self.headers["Authorization"]})
            with urllib.request.urlopen(request, timeout=10) as response:
                body = response.read()
                self.send_response(response.status)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
        def do_POST(self):
            calls.append(self.path)
            body = self.rfile.read(int(self.headers["Content-Length"]))
            request = urllib.request.Request(origin + self.path, data=body, headers={
                "Authorization": self.headers["Authorization"],
                "Content-Type": self.headers["Content-Type"], "X-Beweft-Request": "1",
            })
            with urllib.request.urlopen(request, timeout=10) as response:
                assert response.status == 200
                response.read()
            self.connection.shutdown(socket.SHUT_RDWR)
            self.connection.close()
    proxy = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Proxy)
    thread = threading.Thread(target=proxy.serve_forever, daemon=True)
    thread.start()
    mcp = Mcp(dict(env, BEWEFT_URL=f"http://127.0.0.1:{proxy.server_port}"))
    try:
        mcp.initialize()
        error = mcp.call("media_upload", {"brand_id": brand, "path": str(path)}, error=True)
        assert "inspect media before retrying" in error and calls == ["/api/v1/media?brand_id=" + brand]
    finally:
        mcp.close()
        proxy.shutdown()
        proxy.server_close()
        thread.join(timeout=5)


def pagination_test(env, unrestricted_env, call, brand, other, channel):
    """Read 170 maximum-size drafts, including tied timestamps, exactly once."""
    draft_ids = set()
    for index in range(170):
        text = f"{index:04d}" + "x" * 99996
        draft = call("/posts", {"brand_id": brand, "title": "Pagination fixture", "text": text})
        draft_ids.add(draft["id"])
    outside = call("/posts", {"brand_id": other, "text": "outside scope"})["id"]
    subprocess.run(["docker", "exec", CONTAINER, "psql", "-U", DB_USER, "-d", DB, "-v", "ON_ERROR_STOP=1", "-c",
                    "UPDATE posts SET created_at='2098-12-31T00:00:00Z' WHERE title='Pagination fixture'"],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    queued, history = set(), set()
    for index in range(24):
        post = call("/posts", {"brand_id": brand, "text": f"queue fixture {index}"})
        scheduled_at = (dt.datetime(2099, 1, 2, tzinfo=dt.timezone.utc) + dt.timedelta(minutes=index)).isoformat()
        publication = call(f"/posts/{post['id']}/schedule", {"version": post["version"], "channel_ids": [channel], "scheduled_at": scheduled_at})[0]
        if index % 2:
            call(f"/publications/{publication['id']}/cancel", {})
            history.add(publication["id"])
        else:
            queued.add(publication["id"])
    other_channel = call("/channels", {"brand_id": other, "name": "Other manual", "provider": "manual"})
    other_post = call("/posts", {"brand_id": other, "text": "outside queue scope"})
    other_publication = call(f"/posts/{other_post['id']}/schedule", {"version": other_post["version"], "channel_ids": [other_channel["id"]], "scheduled_at": "2099-01-03T00:00:00Z"})[0]["id"]
    mcp = Mcp(env)
    try:
        mcp.initialize()
        first = mcp.call("drafts", {"brand_id": brand, "limit": 10})
        cursor = first["next_cursor"]
        assert cursor and "400" in mcp.call("drafts", {"limit": 10, "cursor": cursor}, error=True)
        assert "400" in mcp.call("media", {"brand_id": brand, "limit": 10, "cursor": cursor}, error=True)
        assert "403" in mcp.call("drafts", {"brand_id": other, "cursor": cursor}, error=True)
        assert "HTTP 400" in cli(env, "api", "GET", "/posts?limit=10&status=draft&view=summary&cursor=bad", success=False)
        assert "HTTP 400" in cli(env, "api", "GET", "/posts?limit=10&brand_id=" + brand + "&view=summary&cursor=" + cursor, success=False)
        assert "HTTP 400" in cli(env, "api", "GET", "/posts?limit=10&brand_id=" + brand + "&status=draft&cursor=" + cursor, success=False)
        assert "HTTP 400" in cli(env, "api", "GET", "/posts?limit=11", success=False)
        rows = list(first["data"])
        while cursor:
            page = mcp.call("drafts", {"brand_id": brand, "limit": 10, "cursor": cursor})
            rows.extend(page["data"])
            cursor = page["next_cursor"]
            assert len(rows) < 250
        ids = [row["id"] for row in rows]
        assert len(ids) == len(set(ids)) and draft_ids.issubset(ids) and outside not in ids
        assert all(row["status"] == "draft" and "variants" not in row and "options" not in row for row in rows)
        assert len([item for item in ids if item in draft_ids]) == 170
        for tool, expected in (("queue", queued), ("history", history)):
            rows, cursor = [], None
            while True:
                args = {"brand_id": brand, "limit": 3}
                if cursor:
                    args["cursor"] = cursor
                page = mcp.call(tool, args)
                if tool == "queue" and not rows and page["next_cursor"]:
                    assert "400" in mcp.call("history", {"brand_id": brand, "limit": 3, "cursor": page["next_cursor"]}, error=True)
                rows.extend(page["data"])
                cursor = page["next_cursor"]
                if not cursor:
                    break
            ids = [row["id"] for row in rows]
            assert len(ids) == len(set(ids)) and expected.issubset(ids)
            assert all((row["state"] in ("queued", "sending")) == (tool == "queue") and "payload" not in row for row in rows)
        detail = mcp.call("publication", {"id": next(iter(queued))})["data"]
        assert detail["payload"]["text"].startswith("queue fixture ")
        assert other_publication not in [row["id"] for row in mcp.call("queue")["data"]]
        assert "inaccessible" in mcp.call("publication", {"id": other_publication}, error=True)
        other_mcp = Mcp(unrestricted_env)
        try:
            other_mcp.initialize()
            assert "400" in other_mcp.call("drafts", {"brand_id": brand, "limit": 10, "cursor": first["next_cursor"]}, error=True)
        finally:
            other_mcp.close()
    finally:
        mcp.close()


def request_bounds_test(env, call, brand, initial_channel):
    channels = [initial_channel]
    for index in range(49):
        channels.append(call("/channels", {"brand_id": brand, "name": f"Bounded fixture {index}", "provider": "manual"})["id"])

    # JSON escapes make each valid 100,000-byte text about 600 KB on the wire.
    # Returning 50 full snapshots would exceed the CLI's 16 MiB response cap.
    content = "\x01" * 100000
    post = call("/posts", {"brand_id": brand, "text": content})
    schedule_body = {"version": post["version"], "channel_ids": channels, "scheduled_at": "2099-02-01T00:00:00Z"}
    error = cli(env, "api", "POST", f"/posts/{post['id']}/schedule?view=invalid", "--json", "-", body=schedule_body, success=False)
    assert "HTTP 400" in error
    assert call("/posts/" + post["id"])["status"] == "draft"
    assert not any(row["post_id"] == post["id"] for row in call("/publications"))
    mcp = Mcp(env)
    try:
        mcp.initialize()
        result = mcp.call("schedule", {"id": post["id"], "intent": "exact", "body": schedule_body})["data"]
        ids = {row["id"] for row in result}
        assert len(result) == len(ids) == 50
        assert all(row["state"] == "queued" and "payload" not in row for row in result)
        found, cursor = set(), None
        while True:
            args = {"brand_id": brand, "limit": 10}
            if cursor:
                args["cursor"] = cursor
            page = mcp.call("queue", args)
            found.update(row["id"] for row in page["data"] if row["post_id"] == post["id"])
            cursor = page["next_cursor"]
            if not cursor:
                break
        assert found == ids
        detail = mcp.call("publication", {"id": result[0]["id"]})["data"]
        assert detail["payload"]["text"] == content

        # A valid near-1-MiB API JSON body must fit inside the larger CLI/MCP
        # envelope. Each variant remains below the API's per-text limit.
        body = {"brand_id": brand, "text": "envelope boundary", "variants": [{"channel_id": channel, "text": ""} for channel in channels[:11]]}
        target = 1_048_520
        size = lambda: len(json.dumps(body, separators=(",", ":")).encode())
        remaining = target - size()
        for variant in body["variants"]:
            count = min(100000, remaining)
            variant["text"] = "x" * count
            remaining -= count
        assert remaining == 0 and size() == target
        args = {"body": body}
        assert len(json.dumps(args).encode()) > 1024 * 1024
        frame = {"jsonrpc": "2.0", "id": "boundary", "method": "tools/call", "params": {"name": "draft_create", "arguments": args}}
        assert len(json.dumps(frame).encode()) > 1024 * 1024
        assert cli(env, "draft-create", "--json", "-", body=args)["data"]["status"] == "draft"
        assert mcp.call("draft_create", args)["data"]["status"] == "draft"

        # Python's default JSON encoder escapes non-BMP characters as UTF-16
        # surrogate pairs, making the wire envelope much larger than the UTF-8
        # API body that the CLI actually sends.
        emoji = "😀" * 22000
        emoji_body = {"brand_id": brand, "text": "escaped emoji", "variants": [{"channel_id": channel, "text": emoji} for channel in channels[:11]]}
        emoji_args = {"body": emoji_body}
        api_size = len(json.dumps(emoji_body, ensure_ascii=False, separators=(",", ":")).encode())
        wire = json.dumps(emoji_args).encode()
        frame = {"jsonrpc": "2.0", "id": "emoji", "method": "tools/call", "params": {"name": "draft_create", "arguments": emoji_args}}
        assert api_size < 1024 * 1024 and 2 * 1024 * 1024 < len(wire) < 8 * 1024 * 1024
        assert len(json.dumps(frame).encode()) > 2 * 1024 * 1024
        emoji_cli = cli(env, "draft-create", "--json", "-", body=emoji_args)["data"]
        emoji_mcp = mcp.call("draft_create", emoji_args)["data"]
        assert emoji_cli["variants"][0]["text"] == emoji
        assert mcp.call("post", {"id": emoji_mcp["id"]})["data"]["variants"][-1]["text"] == emoji

        # Compact JSON can encode every ASCII byte as a six-byte \u escape.
        ascii_text = "x" * 90000
        ascii_body = {"brand_id": brand, "text": "compact escape", "variants": [{"channel_id": channel, "text": ascii_text} for channel in channels[:11]]}
        ascii_args = {"body": ascii_body}
        api_size = len(json.dumps(ascii_body, separators=(",", ":")).encode())
        escaped = json.dumps(ascii_args, separators=(",", ":")).encode().replace(b"x", b"\\u0078")
        frame = {"jsonrpc": "2.0", "id": "ascii-escaped", "method": "tools/call", "params": {"name": "draft_create", "arguments": ascii_args}}
        escaped_frame = json.dumps(frame, separators=(",", ":")).encode().replace(b"x", b"\\u0078")
        assert api_size < 1024 * 1024 and 5 * 1024 * 1024 < len(escaped) < 8 * 1024 * 1024
        assert 5 * 1024 * 1024 < len(escaped_frame) < 8 * 1024 * 1024
        raw_cli = subprocess.run([str(BIN / "beweft"), "draft-create", "--json", "-"], env=env, input=escaped, capture_output=True, timeout=30)
        assert raw_cli.returncode == 0 and json.loads(raw_cli.stdout)["data"]["variants"][0]["text"] == ascii_text
        mcp.send(escaped_frame)
        raw_mcp = mcp.receive()
        assert raw_mcp["id"] == "ascii-escaped" and not raw_mcp["result"]["isError"]
        ascii_id = raw_mcp["result"]["structuredContent"]["data"]["id"]
        assert mcp.call("post", {"id": ascii_id})["data"]["variants"][-1]["text"] == ascii_text
        too_large = {"body": {"brand_id": brand, "text": "API body cap", "variants": [{"channel_id": channel, "text": "x" * 100000} for channel in channels[:11]]}}
        assert "HTTP 413" in cli(env, "draft-create", "--json", "-", body=too_large, success=False)
        assert "HTTP 413" in mcp.call("draft_create", too_large, error=True)
    finally:
        mcp.close()
    error = cli(env, "draft-create", "--json", "-", body={"body": {"text": "x" * (8 * 1024 * 1024)}}, success=False)
    assert "JSON input exceeds 8 MiB" in error
    oversized = subprocess.run([str(BIN / "beweft"), "mcp"], env=env, input=b" " * (8 * 1024 * 1024 + 1) + b"\n", capture_output=True, timeout=30)
    response = json.loads(oversized.stdout)
    assert oversized.returncode == 0 and response["error"]["code"] == -32600 and "8 MiB" in response["error"]["message"]


def run(temporary):
    port = unused_port()
    origin = f"http://127.0.0.1:{port}"
    temp = Path(temporary)
    media, upload_root = temp / "media", temp / "uploads"
    media.mkdir()
    upload_root.mkdir()
    clean_env = {k: v for k, v in os.environ.items() if not k.startswith("BEWEFT_") and k != "DATABASE_URL"}
    server_env = dict(clean_env, DATABASE_URL=f"postgres://{DB_USER}:{DB_PASSWORD}@127.0.0.1:{DB_PORT}/{DB}",
                      BEWEFT_PUBLIC_URL=origin, BEWEFT_BIND=f"127.0.0.1:{port}", BEWEFT_MEDIA_DIR=str(media),
                      BEWEFT_SETUP_SECRET="isolated-cli-setup-secret", BEWEFT_ENABLE_FIXTURES="false")
    server = subprocess.Popen([str(BIN / "beweft-server")], cwd=ROOT, env=server_env,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
    def call(path, body=None, token=None):
        headers = {"X-Beweft-Request": "1", "Origin": origin}
        if body is not None:
            headers["Content-Type"] = "application/json"
        if token:
            headers["Authorization"] = "Bearer " + token
        request = urllib.request.Request(origin + "/api/v1" + path,
                                         data=None if body is None else json.dumps(body).encode(), headers=headers)
        with client.open(request, timeout=10) as response:
            return json.load(response)["data"]
    try:
        for _ in range(100):
            try:
                call("/auth/status")
                break
            except (OSError, urllib.error.URLError):
                assert server.poll() is None, "Isolated API exited during startup"
                time.sleep(0.1)
        else:
            raise AssertionError("Isolated API did not start")
        call("/auth/bootstrap", {"setup_secret": server_env["BEWEFT_SETUP_SECRET"], "email": "cli@example.invalid", "password": "Isolated-cli-password!"})
        brand = call("/brands", {"name": "CLI fixture"})
        other = call("/brands", {"name": "Outside scope"})
        channel = call("/channels", {"brand_id": brand["id"], "name": "Manual fixture", "provider": "manual"})
        read = call("/tokens", {"name": "Read fixture", "scopes": ["read"], "brand_ids": [brand["id"]]})["token"]
        write = call("/tokens", {"name": "Write fixture", "scopes": ["read", "drafts:write", "publishing:write"], "brand_ids": [brand["id"]]})["token"]
        unrestricted = call("/tokens", {"name": "Unrestricted read fixture", "scopes": ["read"]})["token"]
        other_write = call("/tokens", {"name": "Other brand fixture", "scopes": ["read", "drafts:write"], "brand_ids": [other["id"]]})["token"]
        env = dict(clean_env, BEWEFT_URL=origin, BEWEFT_TOKEN=write,
                   BEWEFT_UPLOAD_ROOT=str(upload_root), BEWEFT_MCP_CALLS_PER_MINUTE="120")
        read_env = dict(env, BEWEFT_TOKEN=read)
        assert [item["id"] for item in cli(read_env, "brands")["data"]] == [brand["id"]]
        assert "403" in cli(read_env, "draft-create", "--json", "-", body={"body": {"brand_id": brand["id"], "text": "denied"}}, success=False)
        assert "HTTP 422" in cli(env, "draft-create", "--json", "-", body={"body": {"brand_id": brand["id"], "text": 123}}, success=False)
        assert "403" in cli(env, "draft-create", "--json", "-", body={"body": {"brand_id": other["id"], "text": "denied"}}, success=False)
        assert "403" in cli(read_env, "channels", "--json", "-", body={"brand_id": other["id"]}, success=False)
        for path in ("https://evil.invalid", "//evil.invalid", "/../auth", "/%2e%2e/auth", "/x\\y", "/x#fragment", "/\n"):
            cli(env, "api", "GET", path, success=False)
        assert cli(read_env, "api", "GET", "/posts?brand_id=" + brand["id"])["data"] == []
        assert "HTTPS" in cli(dict(env, BEWEFT_URL="http://example.invalid"), "brands", success=False)
        token_file = temp / "token"
        token_file.write_text(write)
        token_file.chmod(0o644)
        file_env = dict(env)
        del file_env["BEWEFT_TOKEN"]
        file_env["BEWEFT_TOKEN_FILE"] = str(token_file)
        assert "chmod 600" in cli(file_env, "brands", success=False)
        token_file.chmod(0o600)
        assert cli(file_env, "brands")["data"][0]["id"] == brand["id"]
        assert "only one" in cli(dict(file_env, BEWEFT_TOKEN=write), "brands", success=False)
        created = cli(env, "draft-create", "--json", "-", body={"body": {"brand_id": brand["id"], "text": "CLI draft", "options": {"source": "fixture"}, "variants": [{"channel_id": channel["id"], "text": "fixture variant"}]}})["data"]
        assert created["status"] == "draft" and not cli(env, "queue")["data"]
        summary = cli(env, "drafts")["data"][0]
        assert summary["id"] == created["id"] and "variants" not in summary and "options" not in summary
        full = cli(env, "post", "--json", "-", body={"id": created["id"]})["data"]
        assert full["options"]["source"] == "fixture" and full["variants"][0]["text"] == "fixture variant"
        with client.open(origin + "/api/v1/posts?brand_id=" + brand["id"], timeout=10) as response:
            legacy = json.load(response)
            assert "next_cursor" not in legacy and legacy["data"][0]["variants"][0]["text"] == "fixture variant"
        updated = cli(env, "draft-update", "--json", "-", body={"id": created["id"], "body": {"version": created["version"], "text": "CLI revised"}})["data"]
        assert cli(env, "post", "--json", "-", body={"id": created["id"]})["data"]["text"] == "CLI revised"
        schedule = {"id": created["id"], "intent": "exact", "body": {"version": updated["version"], "channel_ids": [channel["id"]], "scheduled_at": "2099-01-01T00:00:00Z"}}
        assert "scheduled_at" in cli(env, "schedule", "--json", "-", body={**schedule, "intent": "queue"}, success=False)
        assert "403" in cli(read_env, "schedule", "--json", "-", body=schedule, success=False)
        publication = cli(env, "schedule", "--json", "-", body=schedule)["data"][0]
        assert "payload" not in publication
        assert cli(env, "queue")["data"][0]["id"] == publication["id"]
        with client.open(origin + "/api/v1/publications?brand_id=" + brand["id"], timeout=10) as response:
            legacy = json.load(response)
            assert "next_cursor" not in legacy and legacy["data"][0]["payload"]["text"] == "fixture variant"
        assert not cli(env, "attempts", "--json", "-", body={"id": publication["id"]})["data"]
        assert not cli(env, "history")["data"] and not cli(env, "health")["data"]["worker_healthy"]
        lost_response_test(env, origin, brand["id"])
        mcp = Mcp(env)
        try:
            mcp.send(b"{")
            assert mcp.receive()["error"]["code"] == -32700
            assert mcp.request("tools/list")["error"]["code"] == -32002
            mcp.initialize("2099-01-01")
            tools = {tool["name"]: tool for tool in mcp.request("tools/list")["result"]["tools"]}
            assert tools["draft_create"]["annotations"]["readOnlyHint"] is False
            assert tools["brands"]["annotations"]["readOnlyHint"] is True
            assert tools["schedule"]["annotations"]["openWorldHint"] is True
            assert tools["draft_create"]["inputSchema"]["properties"]["body"]["required"]
            assert mcp.request("tools/call", {"name": "unknown"})["error"]["code"] == -32602
            assert mcp.request("tools/call", {"name": "brands", "arguments": []})["error"]["code"] == -32602
            mcp.send({"jsonrpc": "2.0", "method": "tools/call", "params": {"name": "draft_create", "arguments": {"body": {"brand_id": brand["id"], "text": "notification must not write"}}}})
            draft = mcp.call("draft_create", {"body": {"brand_id": brand["id"], "text": "MCP draft"}})["data"]
            assert draft["status"] == "draft"
            assert mcp.call("post", {"id": draft["id"]})["data"]["text"] == "MCP draft"
            assert not any(post["text"] == "notification must not write" for post in mcp.call("drafts")["data"])
            assert "Missing argument" in mcp.call("post", {}, error=True)
            outside = temp / "outside.png"
            outside.write_bytes(b"outside")
            link = upload_root / "link.png"
            link.symlink_to(outside)
            assert "outside" in mcp.call("media_upload", {"brand_id": brand["id"], "path": str(outside)}, error=True)
            assert "symlinks" in mcp.call("media_upload", {"brand_id": brand["id"], "path": str(link)}, error=True)
            image = upload_root / "valid.png"
            image_bytes = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Zl1sAAAAASUVORK5CYII=")
            image.write_bytes(image_bytes)
            uploaded = mcp.call("media_upload", {"brand_id": brand["id"], "path": str(image)})["data"]
            req = urllib.request.Request(origin + uploaded["url"], headers={"Authorization": "Bearer " + write})
            with urllib.request.urlopen(req, timeout=10) as response:
                assert response.read() == image_bytes
            for _ in range(9):
                mcp.call("media_upload", {"brand_id": brand["id"], "path": str(image)})
            media_rows, media_cursor = [], None
            while True:
                args = {"brand_id": brand["id"], "limit": 3}
                if media_cursor:
                    args["cursor"] = media_cursor
                page = mcp.call("media", args)
                media_rows.extend(page["data"])
                media_cursor = page["next_cursor"]
                if not media_cursor:
                    break
            media_ids = [row["id"] for row in media_rows]
            assert len(media_ids) == 10 and len(set(media_ids)) == 10 and all(row["brand_id"] == brand["id"] for row in media_rows)
            lost_image = upload_root / "lost-response.png"
            lost_image.write_bytes(image_bytes)
            lost_upload_response_test(env, origin, brand["id"], lost_image)
            other_image = upload_root / "outside-brand.png"
            other_image.write_bytes(image_bytes)
            cli(dict(env, BEWEFT_TOKEN=other_write), "media-upload", "--json", "-", body={"brand_id": other["id"], "path": str(other_image)})
            recovered = mcp.call("media", {"brand_id": brand["id"]})["data"]
            assert len([row for row in recovered if row["name"] == lost_image.name]) == 1
            assert not any(row["name"] == other_image.name for row in mcp.call("media")["data"])
            assert "403" in mcp.call("media", {"brand_id": other["id"]}, error=True)
            recovered_id = next(row["id"] for row in recovered if row["name"] == lost_image.name)
            req = urllib.request.Request(origin + "/api/v1/media/" + recovered_id + "/file", headers={"Authorization": "Bearer " + write})
            with urllib.request.urlopen(req, timeout=10) as response:
                assert response.read() == image_bytes
        finally:
            mcp.close()
        limited = Mcp(dict(read_env, BEWEFT_MCP_CALLS_PER_MINUTE="1"))
        try:
            limited.initialize()
            limited.call("brands")
            assert "rate limit" in limited.call("brands", error=True)
        finally:
            limited.close()
        restricted = Mcp(read_env)
        try:
            restricted.initialize()
            assert "403" in restricted.call("draft_create", {"body": {"brand_id": brand["id"], "text": "denied"}}, error=True)
            assert "403" in restricted.call("channels", {"brand_id": other["id"]}, error=True)
        finally:
            restricted.close()
        redirect_test(env)
        race_test(env, upload_root, temp / "race-secret", brand["id"])
        pagination_test(env, dict(env, BEWEFT_TOKEN=unrestricted), call, brand["id"], other["id"], channel["id"])
        request_bounds_test(env, call, brand["id"], channel["id"])
        print("PASS: isolated API CLI/MCP, scoped access, lost-response recovery, 170 large-draft pagination, 50-channel compact scheduling, escaped emoji and ASCII wire envelopes, oversized input rejection, upload confinement, token files, redirects, protocol errors and rate limit")
    finally:
        stop(server)


if __name__ == "__main__":
    subprocess.run(["docker", "exec", CONTAINER, "createdb", "-U", DB_USER, DB], check=True)
    try:
        with tempfile.TemporaryDirectory(prefix="beweft-cli-integration-") as temporary:
            run(temporary)
    finally:
        subprocess.run(["docker", "exec", CONTAINER, "dropdb", "-U", DB_USER, "--force", DB], check=True)
