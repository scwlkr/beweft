#!/usr/bin/env python3
"""Exercise the actual CLI and MCP subprocess against an isolated API/database.

Build first: cargo build -p beweft-server --bin beweft-server -p beweft-cli
Run: python3 crates/cli/tests/integration.py
Uses a uniquely named database in the configured test PostgreSQL container and
removes that database and its temporary files on exit. No worker or provider is run.
"""
import base64
import datetime as dt
import hashlib
import http.client
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


def sql(statement):
    subprocess.run(["docker", "exec", "-i", CONTAINER, "psql", "-U", DB_USER,
                    "-d", DB, "-v", "ON_ERROR_STOP=1"], input=statement,
                   text=True, capture_output=True, check=True)


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


def reflected_error_test(env):
    """A remote error body must never be echoed, even for an allowlisted code."""
    marker = "remote-error-reflection-marker"
    class Host(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass
        def respond(self, code, status=400):
            token = self.headers["Authorization"]
            body = json.dumps({"error": {"code": code, "message": marker + " " + token}}).encode()
            self.send_response(status)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        def do_GET(self):
            self.respond("untrusted-" + self.headers["Authorization"])
        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            if body.get("text") == "unknown":
                self.respond("untrusted-" + self.headers["Authorization"])
            elif body.get("text") == "wrong-status":
                self.respond("post_text_too_long", status=500)
            else:
                self.respond("post_text_too_long")
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Host)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    test_env = dict(env, BEWEFT_URL=f"http://127.0.0.1:{server.server_port}", BEWEFT_TOKEN="isolated-reflection-token")
    args = {"body": {"brand_id": "00000000-0000-0000-0000-000000000001", "text": "fixture"}}
    unknown_args = {"body": dict(args["body"], text="unknown")}
    wrong_status_args = {"body": dict(args["body"], text="wrong-status")}
    mcp = Mcp(test_env)
    try:
        unknown = cli(test_env, "brands", success=False)
        known = cli(test_env, "draft-create", "--json", "-", body=args, success=False)
        assert marker not in unknown and marker not in known and "100,000 bytes" in known
        assert "HTTP 400" in unknown and "outcome may be unknown" not in unknown
        unknown_write = cli(test_env, "draft-create", "--json", "-", body=unknown_args, success=False)
        wrong_status = cli(test_env, "draft-create", "--json", "-", body=wrong_status_args, success=False)
        assert marker not in unknown_write and "outcome may be unknown" in unknown_write
        assert "HTTP 500" in wrong_status and "100,000 bytes" not in wrong_status and "authoritative state" in wrong_status
        mcp.initialize()
        unknown = mcp.call("brands", error=True)
        known = mcp.call("draft_create", args, error=True)
        assert marker not in unknown and marker not in known and "100,000 bytes" in known
        assert "HTTP 400" in unknown and "outcome may be unknown" not in unknown
        unknown_write = mcp.call("draft_create", unknown_args, error=True)
        wrong_status = mcp.call("draft_create", wrong_status_args, error=True)
        assert marker not in unknown_write and "outcome may be unknown" in unknown_write
        assert "HTTP 500" in wrong_status and "100,000 bytes" not in wrong_status and "authoritative state" in wrong_status
    finally:
        mcp.close()
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def wait_for_upload_cleanup(call, media_dir, before, existing_files):
    for _ in range(100):
        limits = call("/media/limits")
        if (limits["reserved_bytes"] == before["reserved_bytes"]
                and limits["used_bytes"] == before["used_bytes"]
                and set(media_dir.iterdir()) == existing_files):
            return
        time.sleep(0.05)
    raise AssertionError("Upload reservation or temporary file was not cleaned up")


def actionable_errors_test(env, read_env, call, brand, channel, upload_root, media_dir):
    """Real API rejections produce fixed, useful CLI and MCP guidance."""
    mcp = Mcp(env)
    restricted = Mcp(read_env)
    try:
        mcp.initialize()
        restricted.initialize()
        too_long = {"body": {"brand_id": brand, "text": "x" * 100001}}
        for error in (cli(env, "draft-create", "--json", "-", body=too_long, success=False), mcp.call("draft_create", too_long, error=True)):
            assert "HTTP 400" in error and "100,000 bytes" in error and "outcome may be unknown" not in error
        scope_error = restricted.call("draft_create", {"body": {"brand_id": brand, "text": "denied"}}, error=True)
        assert "HTTP 403" in scope_error and "scope" in scope_error and "brand" in scope_error

        bad_file = upload_root / "unsupported.bin"
        bad_file.write_bytes(b"x" * 64)
        upload = {"brand_id": brand, "path": str(bad_file)}
        before = call("/media/limits")
        existing_files = set(media_dir.iterdir())
        for upload_call in (lambda: cli(env, "media-upload", "--json", "-", body=upload, success=False),
                            lambda: mcp.call("media_upload", upload, error=True)):
            error = upload_call()
            assert "HTTP 400" in error and "JPEG" in error and "MP4" in error and "outcome may be unknown" not in error
            wait_for_upload_cleanup(call, media_dir, before, existing_files)

        post = call("/posts", {"brand_id": brand, "text": "Needs posting rule"})
        schedule = {"id": post["id"], "intent": "queue", "body": {"version": post["version"], "channel_ids": [channel]}}
        for error in (cli(env, "schedule", "--json", "-", body=schedule, success=False), mcp.call("schedule", schedule, error=True)):
            assert "HTTP 400" in error and "posting schedule" in error and "exact" in error and "outcome may be unknown" not in error
        assert call("/posts/" + post["id"])["status"] == "draft"
        assert not any(row["post_id"] == post["id"] for row in call("/publications"))

        # The first channel reserves inside the transaction; a later channel
        # without a rule must roll back that reservation before this rejection.
        pair = [call("/channels", {"brand_id": brand, "name": f"Rollback fixture {i}", "provider": "manual"})["id"] for i in range(2)]
        pair.sort()
        call("/schedules", {"channel_id": pair[0], "days": [1, 2, 3, 4, 5, 6, 7], "time": "00:01", "timezone": "UTC"})
        rollback_post = call("/posts", {"brand_id": brand, "text": "Rollback schedule fixture"})
        rollback_args = {"id": rollback_post["id"], "intent": "queue", "body": {"version": rollback_post["version"], "channel_ids": pair}}
        assert "posting schedule" in mcp.call("schedule", rollback_args, error=True)
        assert not any(row["post_id"] == rollback_post["id"] for row in call("/publications"))
        assert call("/posts/" + rollback_post["id"])["status"] == "draft"

        edited = cli(env, "draft-update", "--json", "-", body={"id": post["id"], "body": {"version": post["version"], "text": "Updated"}})["data"]
        stale = {"id": post["id"], "body": {"version": post["version"], "text": "Stale"}}
        for error in (cli(env, "draft-update", "--json", "-", body=stale, success=False), mcp.call("draft_update", stale, error=True)):
            assert "HTTP 409" in error and "version" in error and "read" in error.lower()
        assert mcp.call("post", {"id": post["id"]})["data"]["version"] == edited["version"]
        cursor_error = mcp.call("drafts", {"limit": 10, "cursor": "bad"}, error=True)
        assert "HTTP 400" in cursor_error and "cursor" in cursor_error.lower() and "restart" in cursor_error.lower()
    finally:
        mcp.close()
        restricted.close()


def large_unsupported_upload_test(env, call, brand, upload_root, media_dir):
    """Large invalid streams reject cleanly; a valid stream round-trips byte for byte."""
    path = upload_root / "unsupported-large.bin"
    with path.open("wb") as file:
        file.truncate(64 * 1024 * 1024)
    before = call("/media/limits")
    existing_files = set(media_dir.iterdir())
    media_ids = {row["id"] for row in call("/media")}
    upload = {"brand_id": brand, "path": str(path)}
    mcp = Mcp(env)
    try:
        mcp.initialize()
        for _ in range(5):
            for upload_call in (lambda: cli(env, "media-upload", "--json", "-", body=upload, success=False),
                                lambda: mcp.call("media_upload", upload, error=True)):
                error = upload_call()
                assert "HTTP 400" in error and "JPEG" in error and "MP4" in error, error
                assert "outcome may be unknown" not in error.lower(), error
                wait_for_upload_cleanup(call, media_dir, before, existing_files)
                assert {row["id"] for row in call("/media")} == media_ids

        # Reuse the sparse fixture, changing only its signature. Verify every
        # byte returned by the authenticated file endpoint, then delete it.
        with path.open("r+b") as file:
            file.write(b"\x00\x00\x00\x18ftypisom")
        uploaded = cli(env, "media-upload", "--json", "-", body=upload)["data"]
        assert uploaded["size"] == 64 * 1024 * 1024
        expected = hashlib.sha256()
        with path.open("rb") as file:
            while chunk := file.read(65536):
                expected.update(chunk)
        actual = hashlib.sha256()
        request = urllib.request.Request(env["BEWEFT_URL"] + uploaded["url"],
                                         headers={"Authorization": "Bearer " + env["BEWEFT_TOKEN"]})
        with urllib.request.urlopen(request, timeout=20) as response:
            while chunk := response.read(65536):
                actual.update(chunk)
        assert actual.digest() == expected.digest()
        request = urllib.request.Request(env["BEWEFT_URL"] + "/api/v1/media/" + uploaded["id"],
                                         headers={"Authorization": "Bearer " + env["BEWEFT_TOKEN"]}, method="DELETE")
        with urllib.request.urlopen(request, timeout=10) as response:
            assert response.status == 200
        wait_for_upload_cleanup(call, media_dir, before, existing_files)
        assert {row["id"] for row in call("/media")} == media_ids
    finally:
        mcp.close()
        path.unlink()


def chunked_stream_test(env, origin, call, brand, media_dir):
    """Split a slow chunked signature, then verify admission, bytes and cleanup."""
    image = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Zl1sAAAAASUVORK5CYII=")
    prefix = b'--preflight-fixture\r\nContent-Disposition: form-data; name="file"; filename="split.png"\r\nContent-Type: image/png\r\n\r\n'
    suffix = b"\r\n--preflight-fixture--\r\n"
    url = origin + "/api/v1/media?brand_id=" + brand
    headers = {"Authorization": "Bearer " + env["BEWEFT_TOKEN"], "Content-Type": "multipart/form-data; boundary=preflight-fixture"}
    before = call("/media/limits")
    existing_files = set(media_dir.iterdir())
    empty = urllib.request.Request(url, data=prefix + suffix, headers=headers)
    try:
        urllib.request.urlopen(empty, timeout=10)
        raise AssertionError("Empty upload was accepted")
    except urllib.error.HTTPError as error:
        assert error.code == 400
        assert json.load(error)["error"]["code"] == "invalid_request"
    wait_for_upload_cleanup(call, media_dir, before, existing_files)

    port = int(origin.rsplit(":", 1)[1])
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=15)
    try:
        connection.putrequest("POST", "/api/v1/media?brand_id=" + brand)
        for name, value in headers.items():
            connection.putheader(name, value)
        connection.putheader("Transfer-Encoding", "chunked")
        connection.endheaders()
        def send(data):
            connection.send(f"{len(data):x}\r\n".encode() + data + b"\r\n")
        send(prefix)
        send(image[:3])
        for _ in range(100):
            if call("/media/limits")["reserved_bytes"] > before["reserved_bytes"]:
                break
            time.sleep(0.05)
        else:
            raise AssertionError("Chunked upload did not reserve quota")
        send(image[3:32])
        send(image[32:])
        send(suffix)
        connection.send(b"0\r\n\r\n")
        response = connection.getresponse()
        assert response.status == 200
        uploaded = json.loads(response.read())["data"]
    finally:
        connection.close()
    request = urllib.request.Request(origin + uploaded["url"], headers={"Authorization": headers["Authorization"]})
    with urllib.request.urlopen(request, timeout=10) as response:
        assert response.read() == image
    request = urllib.request.Request(origin + "/api/v1/media/" + uploaded["id"], headers={"Authorization": headers["Authorization"]}, method="DELETE")
    with urllib.request.urlopen(request, timeout=10) as response:
        assert response.status == 200
    wait_for_upload_cleanup(call, media_dir, before, existing_files)


def trailing_multipart_test(env, origin, call, brand, media_dir):
    """Slow extra fields and epilogues must finish before rejection or commit."""
    before = call("/media/limits")
    existing_files = set(media_dir.iterdir())
    media_ids = {row["id"] for row in call("/media")}
    boundary = "trailing-fixture"
    prefix = (f'--{boundary}\r\nContent-Disposition: form-data; name="file"; '
              'filename="fixture.bin"\r\nContent-Type: application/octet-stream\r\n\r\n').encode()
    closing = f"\r\n--{boundary}--\r\n".encode()
    size = 8 * 1024 * 1024
    tail_size = 4 * 1024 * 1024
    block = b"0" * 65536
    port = int(origin.rsplit(":", 1)[1])
    valid_signature = b"\x00\x00\x00\x18ftypisom"
    invalid_signature = b"unsupported!"

    def send_repeated(connection, count, slow=False):
        while count:
            amount = min(len(block), count)
            connection.send(block[:amount])
            count -= amount
            if slow:
                time.sleep(0.001)

    for valid, extra_name in ((False, "metadata"), (True, "metadata"),
                              (True, "file"), (False, None), (True, None)):
        signature = valid_signature if valid else invalid_signature
        if extra_name is None:
            separator = closing
            suffix = b""
        else:
            separator = (f'\r\n--{boundary}\r\nContent-Disposition: form-data; '
                         f'name="{extra_name}"\r\n\r\n').encode()
            suffix = closing
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=20)
        try:
            connection.putrequest("POST", "/api/v1/media?brand_id=" + brand)
            connection.putheader("Authorization", "Bearer " + env["BEWEFT_TOKEN"])
            connection.putheader("Content-Type", "multipart/form-data; boundary=" + boundary)
            connection.putheader("Content-Length", str(len(prefix) + size + len(separator) + tail_size + len(suffix)))
            connection.endheaders()
            connection.send(prefix + signature)
            send_repeated(connection, size - len(signature))
            connection.send(separator + block[:4096])
            assert not select.select([connection.sock], [], [], 0.3)[0], "API responded before the trailing body was sent"
            assert {row["id"] for row in call("/media")} == media_ids, "Media committed before the body ended"
            assert call("/media/limits")["reserved_bytes"] > before["reserved_bytes"]
            send_repeated(connection, tail_size - 4096, slow=True)
            if suffix:
                connection.send(suffix)
            response = connection.getresponse()
            result = json.loads(response.read())
            if extra_name is not None:
                assert response.status == 400 and result["error"]["code"] == "invalid_request", result
            elif not valid:
                assert response.status == 400 and result["error"]["code"] == "unsupported_media_type", result
            else:
                assert response.status == 200, result
                uploaded = result["data"]
                assert uploaded["size"] == size
        finally:
            connection.close()

        if valid and extra_name is None:
            expected = hashlib.sha256(signature)
            remaining = size - len(signature)
            while remaining:
                amount = min(len(block), remaining)
                expected.update(block[:amount])
                remaining -= amount
            actual = hashlib.sha256()
            request = urllib.request.Request(origin + uploaded["url"], headers={"Authorization": "Bearer " + env["BEWEFT_TOKEN"]})
            with urllib.request.urlopen(request, timeout=20) as response:
                while chunk := response.read(65536):
                    actual.update(chunk)
            assert actual.digest() == expected.digest()
            request = urllib.request.Request(origin + "/api/v1/media/" + uploaded["id"],
                                             headers={"Authorization": "Bearer " + env["BEWEFT_TOKEN"]}, method="DELETE")
            with urllib.request.urlopen(request, timeout=10) as response:
                assert response.status == 200
        wait_for_upload_cleanup(call, media_dir, before, existing_files)
        assert {row["id"] for row in call("/media")} == media_ids

    # Reject an extra field preceding the file too.
    leading = (f'--{boundary}\r\nContent-Disposition: form-data; name="other"\r\n\r\nx\r\n').encode()
    body = leading + prefix + valid_signature + closing
    request = urllib.request.Request(origin + "/api/v1/media?brand_id=" + brand, data=body,
                                     headers={"Authorization": "Bearer " + env["BEWEFT_TOKEN"],
                                              "Content-Type": "multipart/form-data; boundary=" + boundary})
    try:
        urllib.request.urlopen(request, timeout=10)
        raise AssertionError("Leading extra field was accepted")
    except urllib.error.HTTPError as error:
        assert error.code == 400 and json.load(error)["error"]["code"] == "invalid_request"
    wait_for_upload_cleanup(call, media_dir, before, existing_files)
    assert {row["id"] for row in call("/media")} == media_ids

    # An oversized chunked epilogue must hit the request body limit before
    # commit, even without Content-Length. The fixture's limit is 65 MiB.
    epilogue_size = 66 * 1024 * 1024
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=20)
    try:
        connection.putrequest("POST", "/api/v1/media?brand_id=" + brand)
        connection.putheader("Authorization", "Bearer " + env["BEWEFT_TOKEN"])
        connection.putheader("Content-Type", "multipart/form-data; boundary=" + boundary)
        connection.putheader("Transfer-Encoding", "chunked")
        connection.endheaders()
        try:
            def send_chunk(data):
                connection.send(f"{len(data):x}\r\n".encode() + data + b"\r\n")
            send_chunk(prefix + valid_signature + closing)
            remaining = epilogue_size
            while remaining:
                amount = min(len(block), remaining)
                send_chunk(block[:amount])
                remaining -= amount
            connection.send(b"0\r\n\r\n")
            response = connection.getresponse()
            assert response.status == 400, response.status
            response.read()
        except (BrokenPipeError, ConnectionResetError, http.client.RemoteDisconnected):
            # A bounded refusal may close the connection while the sender is active.
            pass
    finally:
        connection.close()
    wait_for_upload_cleanup(call, media_dir, before, existing_files)
    assert {row["id"] for row in call("/media")} == media_ids


def bootstrap_commit_401_test(env, setup_secret, call):
    """Force the committed-owner/session-creation race in an isolated database."""
    sql("""
        CREATE FUNCTION cli_bootstrap_hash_change() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN
            IF NEW.email = 'bootstrap-race@example.invalid' THEN
                UPDATE owners SET password_hash = 'isolated-changed-hash' WHERE id = NEW.id;
            END IF;
            RETURN NEW;
        END $$;
        CREATE TRIGGER cli_bootstrap_hash_change AFTER INSERT ON owners
        FOR EACH ROW EXECUTE FUNCTION cli_bootstrap_hash_change();
    """)
    try:
        body = {"setup_secret": setup_secret, "email": "bootstrap-race@example.invalid",
                "password": "Isolated-bootstrap-password!"}
        error = cli(env, "api", "POST", "/auth/bootstrap", "--json", "-", body=body, success=False)
        assert "HTTP 401" in error and "credentials" in error.lower(), error
        assert "outcome may be unknown" in error.lower() and "authoritative state" in error.lower(), error
        assert "token was rejected" not in error.lower(), error
        assert call("/auth/status")["initialized"] is True
    finally:
        # This database is created exclusively for this integration run.
        sql("DROP TRIGGER cli_bootstrap_hash_change ON owners; DROP FUNCTION cli_bootstrap_hash_change(); DELETE FROM owners;")
    assert call("/auth/status")["initialized"] is False


def auth_error_test(env, brand):
    """401 may be a password failure or an invalid bearer, and writes remain uncertain."""
    login = {"email": "cli@example.invalid", "password": "wrong-isolated-password"}
    error = cli(env, "api", "POST", "/auth/login", "--json", "-", body=login, success=False)
    assert "HTTP 401" in error and "credentials" in error.lower(), error
    assert "outcome may be unknown" in error.lower() and "authoritative state" in error.lower(), error
    assert "token was rejected" not in error.lower(), error
    invalid_env = dict(env, BEWEFT_TOKEN="isolated-invalid-token")
    read_error = cli(invalid_env, "brands", success=False)
    assert "HTTP 401" in read_error and "credentials" in read_error.lower(), read_error
    assert "outcome may be unknown" not in read_error.lower(), read_error
    mcp = Mcp(invalid_env)
    try:
        mcp.initialize()
        read_error = mcp.call("brands", error=True)
        write_error = mcp.call("draft_create", {"body": {"brand_id": brand, "text": "rejected"}}, error=True)
        assert "HTTP 401" in read_error and "credentials" in read_error.lower(), read_error
        assert "outcome may be unknown" not in read_error.lower(), read_error
        assert "HTTP 401" in write_error and "credentials" in write_error.lower(), write_error
        assert "outcome may be unknown" in write_error.lower() and "authoritative state" in write_error.lower(), write_error
        assert "token was rejected" not in write_error.lower(), write_error
    finally:
        mcp.close()


def page_limit_error_test(env, origin):
    """Check deserialization errors, parsed range errors and local guidance."""
    cases = [("-1", "invalid_request"), ("4294967296", "invalid_request"),
             ("abc", "invalid_request"), ("0", "invalid_page_limit"),
             ("11", "invalid_page_limit"), ("4294967295", "invalid_page_limit")]
    for route in ("posts", "media", "publications"):
        for value, expected in cases:
            request = urllib.request.Request(origin + "/api/v1/" + route + "?limit=" + value,
                                             headers={"Authorization": "Bearer " + env["BEWEFT_TOKEN"]})
            try:
                urllib.request.urlopen(request, timeout=10)
                raise AssertionError(f"{route} accepted limit={value}")
            except urllib.error.HTTPError as error:
                assert error.code == 400 and json.load(error)["error"]["code"] == expected
            raw_error = cli(env, "api", "GET", f"/{route}?limit={value}", success=False)
            assert "Provide limit once as an integer from 1 to 10" in raw_error, raw_error
        duplicate = cli(env, "api", "GET", f"/{route}?limit=1&limit=2", success=False)
        assert "Provide limit once as an integer from 1 to 10" in duplicate, duplicate
        unrelated = cli(env, "api", "GET", f"/{route}?brand_id=not-a-uuid&limit=1", success=False)
        assert "HTTP 400" in unrelated and "Provide limit" not in unrelated, unrelated
        secret_value = "private-query-value-fixture"
        redacted = cli(env, "api", "GET", f"/{route}?limit={secret_value}", success=False)
        assert secret_value not in redacted and "integer from 1 to 10" in redacted, redacted
    assert cli(env, "api", "GET", "/media/limits?limit=abc")["data"]["max_upload_bytes"] > 0
    assert cli(env, "api", "GET", "/brands?limit=abc")["data"]
    mcp = Mcp(env)
    try:
        mcp.initialize()
        for limit in (-1, 0, 11, 4294967296):
            args = {"limit": limit}
            for error in (cli(env, "drafts", "--json", "-", body=args, success=False),
                          mcp.call("drafts", args, error=True)):
                assert "limit must be between 1 and 10" in error, error
    finally:
        mcp.close()


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
        assert "outcome may be unknown" in error and "inspect media before retrying" in error and calls == ["/api/v1/media?brand_id=" + brand]
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
        assert "integer from 1 to 10" in cli(env, "api", "GET", "/posts?limit=11", success=False)
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
                      BEWEFT_SETUP_SECRET="isolated-cli-setup-secret", BEWEFT_ENABLE_FIXTURES="false",
                      BEWEFT_MAX_UPLOAD_BYTES=str(64 * 1024 * 1024))
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
        bootstrap_env = dict(clean_env, BEWEFT_URL=origin, BEWEFT_TOKEN="isolated-bootstrap-token")
        bootstrap_commit_401_test(bootstrap_env, server_env["BEWEFT_SETUP_SECRET"], call)
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
        auth_error_test(env, brand["id"])
        page_limit_error_test(env, origin)
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
        actionable_errors_test(env, read_env, call, brand["id"], channel["id"], upload_root, media)
        large_unsupported_upload_test(env, call, brand["id"], upload_root, media)
        reflected_error_test(env)
        chunked_stream_test(env, origin, call, brand["id"], media)
        trailing_multipart_test(env, origin, call, brand["id"], media)
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
        print("PASS: isolated API CLI/MCP, full multipart field/epilogue drain and bounds, post-commit 401, redacted errors, large upload cleanup and byte readback, raw limit guidance, schedule rollback, lost-response recovery, scoped pagination, compact scheduling and bounded envelopes")
    finally:
        stop(server)


if __name__ == "__main__":
    subprocess.run(["docker", "exec", CONTAINER, "createdb", "-U", DB_USER, DB], check=True)
    try:
        with tempfile.TemporaryDirectory(prefix="beweft-cli-integration-") as temporary:
            run(temporary)
    finally:
        subprocess.run(["docker", "exec", CONTAINER, "dropdb", "-U", DB_USER, "--force", DB], check=True)
