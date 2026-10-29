#!/usr/bin/env python3
"""Check the real upload timeout without spending the server's 15-minute limit.

Run after `cargo build -p beweft-cli`: python3 crates/cli/tests/slow_upload.py
A loopback fixture delays the response for 125 seconds, beyond the normal
120-second request timeout. It contacts no provider and cleans its own files.
"""
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[3]
BIN = Path(os.environ.get("BEWEFT_TEST_BIN_DIR", ROOT / "target/debug")) / "beweft"
received = []


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def send_json(self, payload):
        body = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        assert self.path == "/api/v1/media/limits"
        self.send_json({"data": {"max_upload_bytes": 1024 * 1024}})

    def do_POST(self):
        received.append(self.rfile.read(int(self.headers["Content-Length"])))
        time.sleep(125)
        self.send_json({"data": {"id": "slow-upload-fixture"}})


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
thread = threading.Thread(target=server.serve_forever, daemon=True)
thread.start()
try:
    with tempfile.TemporaryDirectory(prefix="beweft-slow-upload-") as directory:
        path = Path(directory) / "fixture.png"
        path.write_bytes(b"small fixture")
        env = {key: value for key, value in os.environ.items() if not key.startswith("BEWEFT_")}
        env.update(BEWEFT_URL=f"http://127.0.0.1:{server.server_port}", BEWEFT_TOKEN="isolated-test-token")
        args = {"brand_id": "00000000-0000-0000-0000-000000000001", "path": str(path)}
        print("Waiting 125 seconds for the isolated upload response...", flush=True)
        start = time.monotonic()
        result = subprocess.run([str(BIN), "media-upload", "--json", "-"], input=json.dumps(args),
                                text=True, capture_output=True, env=env, timeout=150)
        elapsed = time.monotonic() - start
        assert result.returncode == 0, result.stderr
        assert json.loads(result.stdout)["data"]["id"] == "slow-upload-fixture"
        assert len(received) == 1 and b"small fixture" in received[0] and elapsed >= 125
        print(f"PASS: upload remained active for {elapsed:.1f}s and was sent once")
finally:
    server.shutdown()
    server.server_close()
    thread.join(timeout=5)
