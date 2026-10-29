#!/usr/bin/env python3
"""Run real browser tests against an owned disposable PostgreSQL database and server.
Requires built Rust binaries, web/dist, and an existing development PostgreSQL container.
No public providers or persistent application database are used.
"""
import http.cookiejar
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[2]
CONTAINER = os.environ.get("BEWEFT_TEST_POSTGRES_CONTAINER", "beweft-dev-postgres")
DB = "beweft_e2e_" + uuid.uuid4().hex[:12]
USER = os.environ.get("BEWEFT_TEST_DB_USER", "beweft")
PASSWORD = os.environ.get("BEWEFT_TEST_DB_PASSWORD", "beweft-local-development")
PORT = os.environ.get("BEWEFT_TEST_DB_PORT", "55439")
processes = []
created = False
temporary = None


def main():
    global created, temporary
    subprocess.run(["docker", "exec", CONTAINER, "createdb", "-U", USER, DB], check=True)
    created = True
    temporary = tempfile.TemporaryDirectory(prefix="beweft-browser-e2e-")
    directory = temporary.name
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    origin = f"http://127.0.0.1:{port}"
    setup = secrets.token_urlsafe(32)
    password = secrets.token_urlsafe(32)
    env = dict(os.environ, DATABASE_URL=f"postgres://{USER}:{PASSWORD}@localhost:{PORT}/{DB}", BEWEFT_PUBLIC_URL=origin, BEWEFT_BIND=f"127.0.0.1:{port}", BEWEFT_MEDIA_DIR=directory + "/media", BEWEFT_WEB_DIR=str(ROOT / "web/dist"), BEWEFT_SETUP_SECRET=setup, BEWEFT_ENCRYPTION_KEY=secrets.token_hex(32), BEWEFT_ENABLE_FIXTURES="true", BEWEFT_TEST_URL=origin, BEWEFT_TEST_ISOLATED="1", BEWEFT_TEST_EMAIL="browser@example.test", BEWEFT_TEST_PASSWORD=password, BEWEFT_TEST_OUTPUT=directory + "/playwright")
    for name in ["beweft-server", "beweft-worker"]:
        processes.append(subprocess.Popen([str(ROOT / "target/debug" / name)], cwd=ROOT, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
    for _ in range(150):
        try:
            with urllib.request.urlopen(origin + "/api/v1/auth/status", timeout=2) as response:
                assert response.status == 200
            break
        except OSError:
            assert all(p.poll() is None for p in processes), "Backend exited during startup"
            time.sleep(.1)
    else:
        raise RuntimeError("Disposable server did not start")
    request = urllib.request.Request(origin + "/api/v1/auth/bootstrap", data=json.dumps({"setup_secret":setup,"email":env["BEWEFT_TEST_EMAIL"],"password":password}).encode(), headers={"Content-Type":"application/json","X-Beweft-Request":"1","Origin":origin})
    with urllib.request.urlopen(request) as response:
        assert response.status == 200
    subprocess.run(["pnpm", "--filter", "@beweft/web", "exec", "playwright", "test", *sys.argv[1:]], cwd=ROOT, env=env, check=True)


try:
    main()
finally:
    for process in processes:
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
    try:
        if created:
            subprocess.run(["docker", "exec", CONTAINER, "dropdb", "-U", USER, "--force", DB], check=True)
    finally:
        if temporary is not None:
            temporary.cleanup()
