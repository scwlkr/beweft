#!/usr/bin/env python3
"""Run a disposable, offline-provider install/recovery/upgrade rehearsal.

Requires an already built BEWEFT_REHEARSAL_IMAGE and the postgres:17-alpine
image. Creates only uniquely named projects, volumes and one derived
image; all are removed in finally. Never contacts a social provider.
"""
import base64
from datetime import datetime, timedelta, timezone
import hashlib
import http.client
import http.cookiejar
import io
import json
import os
from pathlib import Path
import re
import select
import signal
import shutil
import socket
import subprocess
import tarfile
import tempfile
import termios
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[2]
IMAGE = os.environ.get("BEWEFT_REHEARSAL_IMAGE", "beweft:ops-rehearsal-pr18-fix")
CLEAN_ENV = {key: value for key, value in os.environ.items()
             if not key.startswith(("BEWEFT_", "COMPOSE_")) and key != "DATABASE_URL"}
RUN_ID = uuid.uuid4().hex[:12]
PROJECTS = [f"beweft-ops-{RUN_ID}-{letter}" for letter in ("a", "b", "c")]
UPGRADE_IMAGE = f"beweft:ops-upgrade-{RUN_ID}"
BUILDER = f"beweft-ops-image-{RUN_ID}"
MUTABLE_IMAGE = f"beweft:ops-mutable-{RUN_ID}"
POSTGRES_ALIAS = f"beweft:ops-postgres-{RUN_ID}"
POSTGRES_DERIVED = f"beweft:ops-postgres-derived-{RUN_ID}"
POSTGRES_BUILDER = f"beweft-ops-postgres-builder-{RUN_ID}"
HOLD_CONTAINER = f"beweft-ops-hold-{RUN_ID}"
BUILT_IMAGE = None
PASSWORD = "Isolated-operations-owner-password!"
EMAIL = "ops-fixture@example.invalid"
PNG = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=")
SENSITIVE = {PASSWORD}


def run(args, *, success=True, env=None, input=None, timeout=240):
    proc = subprocess.run([str(x) for x in args], cwd=ROOT, env=CLEAN_ENV if env is None else env, input=input,
                          capture_output=True, text=True, timeout=timeout)
    if (proc.returncode == 0) != success:
        diagnostic = proc.stderr[-2000:]
        for secret in SENSITIVE:
            diagnostic = diagnostic.replace(secret, "[redacted]")
        diagnostic = re.sub(r"postgres(?:ql)?://\S+", "[redacted-url]", diagnostic)
        raise AssertionError(f"{Path(str(args[0])).name} exited {proc.returncode}; expected success={success}: {diagnostic}")
    return proc


def docker(*args, **kwargs):
    return run(["docker", *args], **kwargs)


def project(*args, **kwargs):
    return run([ROOT / "project", *args], **kwargs)


def settings(directory):
    values = dict(line.split("=", 1) for line in (directory / "installation.env").read_text().splitlines() if line)
    state = json.loads((directory / "installation-state.json").read_text())
    values.update(BEWEFT_IMAGE=state["image"], BEWEFT_WORKER_ACTIVE="true" if state["worker_active"] else "false")
    SENSITIVE.update(values[key] for key in ("BEWEFT_DB_PASSWORD", "BEWEFT_SETUP_SECRET", "BEWEFT_ENCRYPTION_KEY"))
    return values


def set_values(directory, updates):
    path = directory / "installation.env"
    lines = path.read_text().splitlines()
    found = set()
    for index, line in enumerate(lines):
        key = line.split("=", 1)[0]
        if key in updates:
            lines[index] = key + "=" + updates[key]
            found.add(key)
    assert found == set(updates)
    path.write_text("\n".join(lines) + "\n")
    path.chmod(0o600)


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def no_resources(name):
    for kind in ("container", "volume", "network"):
        command = {"container": ("ps", "-aq"), "volume": ("volume", "ls", "-q"),
                   "network": ("network", "ls", "-q")}[kind]
        assert not docker(*command, "--filter", f"label=com.docker.compose.project={name}").stdout.strip()
    for kind in ("database", "media"):
        docker("volume", "inspect", f"{name}_{kind}", success=False)


class Client:
    def __init__(self, port):
        self.origin = f"http://127.0.0.1:{port}"
        self.client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

    def call(self, path, body=None, expected=200, raw=None, content_type=None):
        headers = {"X-Beweft-Request": "1", "Origin": self.origin}
        if body is not None:
            raw = json.dumps(body).encode()
            content_type = "application/json"
        if content_type:
            headers["Content-Type"] = content_type
        request = urllib.request.Request(self.origin + "/api/v1" + path, data=raw, headers=headers)
        try:
            with self.client.open(request, timeout=15) as response:
                code, data = response.status, response.read()
        except urllib.error.HTTPError as error:
            code, data = error.code, error.read()
        assert code == expected, f"API fixture {path.split('?')[0]} returned HTTP {code}, expected {expected}"
        return json.loads(data).get("data") if code == 200 and data else None

    def media(self, url):
        with self.client.open(self.origin + url, timeout=15) as response:
            return response.read()

    def login(self):
        self.call("/auth/login", {"email": EMAIL, "password": PASSWORD})


def verify_content(client, brand, post, publication, media):
    client.login()
    assert client.call(f"/posts/{post['id']}")["text"] == "Private recovery draft"
    assert client.media(media["url"]) == PNG
    queue = client.call("/publications?brand_id=" + brand["id"])
    found = [entry for entry in queue if entry["id"] == publication["id"]]
    assert len(found) == 1 and found[0]["state"] == "queued"
    assert found[0]["post_id"] == post["id"]


def checksum(backup):
    names = ("database.dump", "media.tar", "key.sha256", "image-id", "postgres-image-id", "manifest")
    (backup / "SHA256SUMS").write_text("".join(
        f"{hashlib.sha256((backup / name).read_bytes()).hexdigest()}  {name}\n" for name in names))


def malicious_backup(source, destination, mode):
    shutil.copytree(source, destination)
    if mode == "checksum":
        with (destination / "database.dump").open("ab") as file:
            file.write(b"tampered")
        return
    with tarfile.open(destination / "media.tar", "w", format=tarfile.USTAR_FORMAT) as archive:
        entry = tarfile.TarInfo({"parent": "../escape", "absolute": "/escape", "link": "link", "hardlink": "link", "device": "device", "duplicate": "same", "pax": "pax"}[mode])
        if mode in ("link", "hardlink"):
            entry.type = tarfile.SYMTYPE if mode == "link" else tarfile.LNKTYPE
            entry.linkname = "../escape"
        elif mode == "device":
            entry.type = tarfile.CHRTYPE
        elif mode == "pax":
            entry.type = tarfile.XHDTYPE
        else:
            entry.size = 1
        archive.addfile(entry, io.BytesIO(b"x") if entry.size else None)
        if mode == "duplicate":
            archive.addfile(entry, io.BytesIO(b"x"))
    checksum(destination)


def image_for(directory, service="api"):
    container = project("compose", directory, "ps", "-q", service).stdout.strip()
    assert container
    return docker("inspect", "--format", "{{.Image}}", container).stdout.strip()


def container_for(directory, service):
    profile = ("--profile", "worker") if service == "worker" else ()
    value = project("compose", directory, *profile, "ps", "-q", service).stdout.strip()
    assert value
    return value


def started_at(container):
    return docker("inspect", "--format", "{{.State.StartedAt}}", container).stdout.strip()


def managed_state_refusal(source, alternate_image):
    """Editing ordinary config or losing state cannot switch the approved image."""
    config = source / "installation.env"
    state = source / "installation-state.json"
    original_config, original_state = config.read_text(), state.read_text()
    api = container_for(source, "api")
    image = image_for(source)
    for line in ("BEWEFT_IMAGE=" + alternate_image, "BEWEFT_WORKER_ACTIVE=true"):
        try:
            config.write_text(original_config + line + "\n")
            config.chmod(0o600)
            project("compose", source, "up", "-d", success=False)
        finally:
            config.write_text(original_config)
            config.chmod(0o600)
    for invalid in (None, "{}\n", '{"image":"unconfigured","worker_active":true,"worker_active":false}\n'):
        try:
            if invalid is None:
                state.unlink()
            else:
                state.write_text(invalid)
                state.chmod(0o600)
            project("compose", source, "up", "-d", success=False)
        finally:
            state.write_text(original_state)
            state.chmod(0o600)
    assert container_for(source, "api") == api and image_for(source) == image


def instrumented_docker(temp):
    """Observe only test-owned helper mounts, or stall one read-only client call."""
    directory = temp / "docker-wrapper"
    directory.mkdir()
    script = directory / "docker"
    real = shutil.which("docker")
    assert real
    script.write_text('''#!/usr/bin/env python3
import json, os, pathlib, subprocess, sys, time
real = %r
args = sys.argv[1:]
stall = os.environ.get("OPS_TEST_STALL")
if stall and not os.environ.get("OPS_TEST_STATE_TARGET") and "compose" in args and args[-3:] == ["ps", "-q", "worker"]:
    pathlib.Path(stall).write_text("waiting")
    time.sleep(90)
state_target = os.environ.get("OPS_TEST_STATE_TARGET")
if stall and state_target and args == ["inspect", "--format", "{{json .State}}", state_target]:
    count_file = pathlib.Path(stall + ".count")
    count = int(count_file.read_text()) + 1 if count_file.exists() else 1
    count_file.write_text(str(count))
    if count == 2:
        pathlib.Path(stall).write_text("waiting after stop")
        time.sleep(90)
delayed_start = os.environ.get("OPS_TEST_START_MARKER")
if delayed_start and "compose" in args and "start" in args:
    delay = os.environ.get("OPS_TEST_START_DELAY", "6")
    program = "import os,sys,time; time.sleep(float(sys.argv[1])); open(sys.argv[2], 'w').write('admitted'); os.execv(sys.argv[3], sys.argv[3:])"
    child = subprocess.Popen([sys.executable, "-c", program, delay, delayed_start, real, *args])
    pathlib.Path(delayed_start + ".pid").write_text(str(child.pid))
    time.sleep(90)
stall_list = os.environ.get("OPS_TEST_STALL_LIST")
if stall_list and len(args) == 4 and args[:3] == ["ps", "-aq", "--filter"] and args[3].startswith("label=com.docker.compose.project="):
    count_file = pathlib.Path(stall_list + ".count")
    count = int(count_file.read_text()) + 1 if count_file.exists() else 1
    count_file.write_text(str(count))
    if count == 2:
        pathlib.Path(stall_list).write_text("waiting")
        time.sleep(90)
stall_suffix = os.environ.get("OPS_TEST_STALL_SUFFIX")
if stall_suffix and args[-len(json.loads(stall_suffix)):] == json.loads(stall_suffix):
    pathlib.Path(os.environ["OPS_TEST_STALL_MARKER"]).write_text("waiting")
    time.sleep(90)
init_mode = os.environ.get("OPS_TEST_INITIALIZER_MODE")
if init_mode == "remove-fail" and len(args) == 3 and args[:2] == ["rm", "-fv"] and "-restore-init-" in args[2]:
    sys.exit(1)
if len(args) >= 3 and args[:2] == ["create", "--name"] and "-restore-" in args[2]:
    name = args[2]
    if "-restore-init-" in name and init_mode == "delay-start":
        args[-1] = "sleep 10; " + args[-1]
    if "-restore-init-" in name and init_mode == "start-fail":
        args[-1] = "exit 7"
    result = subprocess.run([real, *args])
    if result.returncode == 0:
        details = json.loads(subprocess.check_output([real, "inspect", "--format", "{{json .}}", name]))
        with open(os.environ["OPS_TEST_RESTORE_LOG"], "a") as file:
            file.write(json.dumps({"name": name, "labels": details["Config"]["Labels"], "mounts": details["Mounts"],
                                   "configured": details["HostConfig"]["Mounts"]}) + "\\n")
        if "-restore-init-" in name and init_mode in ("delay-create", "delay-create-overdue"):
            pathlib.Path(os.environ["OPS_TEST_INITIALIZER_MARKER"]).write_text(name)
            time.sleep(20 if init_mode == "delay-create-overdue" else 8)
    sys.exit(result.returncode)
kind = os.environ.get("OPS_TEST_PASSTHROUGH")
selected = ((kind == "compose" and "compose" in args and "restart" in args) or
            (kind == "cli" and "compose" in args and "run" in args and "--name" in args) or
            (kind == "build" and args and args[0] == "build") or
            (kind == "restore" and len(args) == 4 and args[:3] == ["start", "--attach", "--interactive"] and "-restore-" in args[3]))
if selected:
    marker = os.environ["OPS_TEST_PASSTHROUGH_MARKER"]
    if kind == "restore":
        # Start the real, uniquely named extractor without attaching its stdin.
        # It remains running while this simulated plugin stops reading the pipe.
        subprocess.run([real, "start", args[3]], check=True, stdout=subprocess.DEVNULL)
    program = ("import os,signal,sys,time; m=sys.argv[1]; "
               "signal.signal(signal.SIGINT,lambda s,f:open(m+'.signal','w').write(str(s))); "
               "signal.signal(signal.SIGTERM,lambda s,f:open(m+'.signal','w').write(str(s))); "
               "open(m+'.ready','w').write('ready'); time.sleep(6); "
               "open(m,'w').write('delayed'); os.execv(sys.argv[2], sys.argv[2:])")
    child = subprocess.Popen([sys.executable, "-c", program, marker, real, *args])
    for _ in range(500):
        if pathlib.Path(marker + ".ready").exists(): break
        if child.poll() is not None: sys.exit(1)
        time.sleep(0.01)
    else: sys.exit(1)
    pathlib.Path(marker + ".args").write_text(json.dumps(args))
    pathlib.Path(marker + ".pid").write_text(str(child.pid))
    time.sleep(90)
if len(args) >= 3 and args[:2] == ["create", "--name"] and "-dump-" in args[2]:
    result = subprocess.run([real, *args])
    if result.returncode == 0:
        name = args[2]
        mounts = json.loads(subprocess.check_output([real, "inspect", "--format", "{{json .Mounts}}", name]))
        image = subprocess.check_output([real, "inspect", "--format", "{{.Image}}", name], text=True).strip()
        labels = json.loads(subprocess.check_output([real, "inspect", "--format", "{{json .Config.Labels}}", name]))
        with open(os.environ["OPS_TEST_DOCKER_LOG"], "a") as file:
            file.write(json.dumps({"name": name, "image": image, "mounts": mounts, "labels": labels}) + "\\n")
    sys.exit(result.returncode)
if len(args) >= 3 and args[:2] == ["create", "--name"] and "-media-" in args[2]:
    result = subprocess.run([real, *args])
    if result.returncode == 0:
        name = args[2]
        details = json.loads(subprocess.check_output([real, "inspect", "--format", "{{json .}}", name]))
        with open(os.environ["OPS_TEST_BACKUP_MEDIA_LOG"], "a") as file:
            file.write(json.dumps({"name": name, "labels": details["Config"]["Labels"], "mounts": details["Mounts"],
                                   "configured": details["HostConfig"]["Mounts"]}) + "\\n")
    sys.exit(result.returncode)
os.execv(real, [real, *args])
''' % real)
    script.chmod(0o755)
    log = temp / "docker-helper-log.jsonl"
    env = dict(CLEAN_ENV, PATH=str(directory) + os.pathsep + CLEAN_ENV.get("PATH", ""),
               OPS_TEST_DOCKER_LOG=str(log), OPS_TEST_RESTORE_LOG=str(temp / "restore-helper-log.jsonl"),
               OPS_TEST_BACKUP_MEDIA_LOG=str(temp / "backup-media-helper-log.jsonl"))
    return env, log


def assert_helpers_removed(log, offset, image):
    records = [json.loads(line) for line in log.read_text().splitlines()[offset:]]
    assert records, "No owned dump helper was observed"
    for record in records:
        assert record["image"] == image, "Dump helper did not use the pinned PostgreSQL image"
        assert re.fullmatch(r"[a-f0-9]{64}", record["labels"]["org.beweft.installation"])
        assert record["labels"]["org.beweft.operation"] == "backup-database"
        assert record["labels"]["org.beweft.operation-id"] == record["name"]
        volumes = [mount["Name"] for mount in record["mounts"] if mount.get("Type") == "volume"]
        assert volumes and all(re.fullmatch(r"[a-f0-9]{64}", volume) for volume in volumes)
        docker("container", "inspect", record["name"], success=False)
        for volume in volumes:
            docker("volume", "inspect", volume, success=False)
    return offset + len(records)


def check_backup_media_helpers(env, directory):
    path = Path(env["OPS_TEST_BACKUP_MEDIA_LOG"])
    records = [json.loads(line) for line in path.read_text().splitlines()]
    values = settings(directory)
    assert records
    for record in records:
        assert record["labels"]["org.beweft.installation"] == values["BEWEFT_INSTALLATION_ID"]
        assert record["labels"]["org.beweft.operation"] == "backup-media"
        assert record["labels"]["org.beweft.operation-id"] == record["name"]
        assert any(mount.get("Target") == "/media" and mount.get("VolumeOptions", {}).get("NoCopy") is True
                   for mount in record["configured"])
        assert any(mount.get("Name") == values["COMPOSE_PROJECT_NAME"] + "_media" and mount.get("Destination") == "/media"
                   for mount in record["mounts"])
        docker("container", "inspect", record["name"], success=False)
        for mount in record["mounts"]:
            if mount.get("Type") == "volume" and mount["Name"] != values["COMPOSE_PROJECT_NAME"] + "_media":
                assert re.fullmatch(r"[a-f0-9]{64}", mount["Name"])
                docker("volume", "inspect", mount["Name"], success=False)
    docker("volume", "inspect", values["COMPOSE_PROJECT_NAME"] + "_media")


def cleanup_process_group(source, temp, env, *, admitted, interrupted=None):
    """A timed-out cleanup must not leave a plugin child that starts the API later."""
    api = container_for(source, "api")
    marker = temp / ("term-start" if interrupted == signal.SIGTERM else "interrupt-start" if interrupted else
                     "admitted-start" if admitted else "delayed-start")
    project("compose", source, "stop", "api")
    assert docker("inspect", "--format", "{{.State.Running}}", api).stdout.strip() == "false"
    code = '''import runpy,signal,sys,time
m=runpy.run_path(sys.argv[1]); install=m["Installation"](sys.argv[2])
previous={s:signal.getsignal(s) for s in (signal.SIGINT,signal.SIGTERM)}
try:
    with install.locked():
        m["restart_original"](install, {}, [time.monotonic()+float(sys.argv[3])])
except m["OperationError"] as error:
    assert all(signal.getsignal(s) == h for s,h in previous.items()), "Signal handlers were not restored"
    print(error, file=sys.stderr)
    sys.exit(1)
raise SystemExit("cleanup unexpectedly succeeded")
'''
    wrapped = dict(env, OPS_TEST_START_MARKER=str(marker), OPS_TEST_START_DELAY="0" if admitted else "6")
    try:
        command = ["python3", "-c", code, str(ROOT / "scripts/self_host.py"), str(source), "30" if interrupted else "3"]
        if interrupted:
            process = subprocess.Popen(command, cwd=ROOT, env=wrapped, stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE, text=True, start_new_session=True)
            try:
                for _ in range(200):
                    if Path(str(marker) + ".pid").exists():
                        break
                    assert process.poll() is None, "Cleanup exited before the plugin descendant started"
                    time.sleep(0.05)
                else:
                    raise AssertionError("The cleanup plugin descendant did not start")
                process.send_signal(interrupted)
                _stdout, stderr = process.communicate(timeout=15)
                assert process.returncode != 0
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
        else:
            stderr = run(command, success=False, env=wrapped, timeout=30).stderr
        assert "daemon" in stderr.lower(), "Uncertain daemon state was not reported"
        assert Path(str(marker) + ".pid").exists(), "The owned plugin descendant was not created"
        time.sleep(7)
        child_pid = int(Path(str(marker) + ".pid").read_text())
        try:
            os.kill(child_pid, 0)
        except ProcessLookupError:
            pass
        else:
            raise AssertionError("The timed-out plugin descendant was not reaped")
        assert marker.exists() == admitted, "A timed-out child executed a delayed mutation"
        running = docker("inspect", "--format", "{{.State.Running}}", api).stdout.strip() == "true"
        assert running == admitted, "The actual Docker state did not match the fixture's admitted request"
    finally:
        project("compose", source, "start", "--wait", "--wait-timeout", "120", "api", timeout=150)


def passthrough_interruptions(source, build_fixture, temp, env):
    """Each inherited-stream path must reap a delayed plugin group on both signals."""
    markers = []
    for kind in ("compose", "cli", "build"):
        for signum in (signal.SIGINT, signal.SIGTERM):
            marker = temp / f"passthrough-{kind}-{signum}"
            wrapped = dict(env, OPS_TEST_PASSTHROUGH=kind, OPS_TEST_PASSTHROUGH_MARKER=str(marker))
            arguments = {"compose": ("compose", source, "restart", "api"),
                         "cli": ("cli", source, "mcp"),
                         "build": ("build", build_fixture)}[kind]
            process = subprocess.Popen([ROOT / "project", *map(str, arguments)], cwd=ROOT, env=wrapped,
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                       start_new_session=True)
            try:
                for _ in range(300):
                    if Path(str(marker) + ".pid").exists():
                        break
                    assert process.poll() is None, f"{kind} exited before its plugin child started"
                    time.sleep(0.05)
                else:
                    raise AssertionError(f"{kind} plugin child did not start")
                if signum == signal.SIGINT:
                    os.killpg(process.pid, signum)
                else:
                    process.send_signal(signum)
                _stdout, stderr = process.communicate(timeout=20)
                assert process.returncode != 0 and b"daemon" in stderr.lower()
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
            markers.append((marker, signum))
    time.sleep(6.5)
    for marker, signum in markers:
        assert not marker.exists(), "An interrupted plugin made a delayed mutation"
        assert Path(str(marker) + ".signal").read_text() == str(signum), "The plugin did not receive cancellation"
        child = int(Path(str(marker) + ".pid").read_text())
        try:
            os.kill(child, 0)
        except ProcessLookupError:
            pass
        else:
            raise AssertionError("An interrupted plugin descendant remained alive")
    assert not docker("ps", "-aq", "--filter", f"name={PROJECTS[0]}-cli-").stdout.strip()


def wait_for_operation(source):
    end = time.monotonic() + 20
    while time.monotonic() < end:
        result = subprocess.run([ROOT / "project", "compose", source, "ps"], cwd=ROOT, env=CLEAN_ENV,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
        if result.returncode == 0:
            return
        time.sleep(0.1)
    raise AssertionError("An admitted live session kept the installation lock")


def live_sessions_release_lock(source, token, backup):
    """A real follower and MCP stdio session must not monopolize the installation."""
    follower = subprocess.Popen([ROOT / "project", "compose", source, "logs", "-f", "--tail", "0", "api"],
                                cwd=ROOT, env=CLEAN_ENV, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                start_new_session=True)
    try:
        time.sleep(1)
        assert follower.poll() is None, "Log follower exited before the concurrent operation"
        wait_for_operation(source)
        assert follower.poll() is None, "Log follower exited during the concurrent operation"
    finally:
        if follower.poll() is None:
            follower.send_signal(signal.SIGTERM)
        follower.communicate(timeout=20)
    assert follower.returncode != 0
    session = subprocess.Popen([ROOT / "project", "cli", source, "mcp"], cwd=ROOT,
                               env=dict(CLEAN_ENV, BEWEFT_TOKEN=token), stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
    try:
        message = {"jsonrpc": "2.0", "id": 1, "method": "initialize",
                   "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                              "clientInfo": {"name": "rehearsal", "version": "1"}}}
        session.stdin.write((json.dumps(message) + "\n").encode())
        session.stdin.flush()
        ready, _, _ = select.select([session.stdout], [], [], 30)
        assert ready, "Scoped MCP session did not produce its protocol response"
        response = json.loads(session.stdout.readline())
        assert response.get("id") == 1 and response.get("result", {}).get("serverInfo", {}).get("name") == "beweft"
        wait_for_operation(source)
        api = container_for(source, "api")
        media_status = ("sh", "-c", 'stat -c "%u:%g" /app/media; ls -A /app/media')
        before = docker("exec", api, *media_status).stdout
        assert before.strip() == "10001:10001", "Fixture media must be empty and app-owned before backup"
        project("backup", source, backup, "--max-downtime", "60", timeout=150)
        assert docker("exec", api, *media_status).stdout == before, "Backup modified the empty media volume"
        assert session.poll() is None, "MCP session exited during the concurrent operation"
        session.stdin.write(b'{"jsonrpc":"2.0","id":2,"method":"ping"}\n')
        session.stdin.flush()
        ready, _, _ = select.select([session.stdout], [], [], 15)
        assert ready and json.loads(session.stdout.readline()).get("id") == 2
        session.send_signal(signal.SIGTERM)
        session.wait(timeout=45)
        session.stdin.close()
        session.stdin = None
        _out, stderr = session.communicate(timeout=25)
        diagnostic = stderr.replace(token.encode(), b"[redacted]")[-1000:]
        assert session.returncode != 0 and b"daemon" in stderr.lower(), f"MCP cancellation returned {session.returncode}: {diagnostic!r}"
        assert not docker("ps", "-aq", "--filter", f"name={PROJECTS[0]}-cli-").stdout.strip(), "Scoped MCP container remained after cancellation"
    finally:
        if session.poll() is None:
            session.kill()
            session.wait(timeout=5)


def interactive_cli_terminal(source, token):
    """A scoped MCP session also receives input and emits JSON on a real PTY."""
    master, slave = os.openpty()
    attributes = termios.tcgetattr(slave)
    attributes[3] &= ~termios.ECHO
    termios.tcsetattr(slave, termios.TCSANOW, attributes)
    process = subprocess.Popen([ROOT / "project", "cli", source, "mcp"], cwd=ROOT,
                               env=dict(CLEAN_ENV, BEWEFT_TOKEN=token), stdin=slave, stdout=slave,
                               stderr=subprocess.PIPE, start_new_session=True)
    os.close(slave)
    try:
        request = {"jsonrpc": "2.0", "id": 7, "method": "initialize",
                   "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                              "clientInfo": {"name": "terminal-fixture", "version": "1"}}}
        os.write(master, (json.dumps(request) + "\n").encode())
        response = b""
        end = time.monotonic() + 30
        while b"\n" not in response and time.monotonic() < end:
            ready, _, _ = select.select([master], [], [], 0.1)
            if ready:
                response += os.read(master, 65536)
            assert process.poll() is None, "Scoped terminal session exited before its response"
        assert b"\n" in response and json.loads(response.splitlines()[0]).get("id") == 7
        process.send_signal(signal.SIGTERM)
        process.wait(timeout=45)
        assert process.returncode != 0
        assert not docker("ps", "-aq", "--filter", f"name={PROJECTS[0]}-cli-").stdout.strip()
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
            try:
                process.wait(timeout=45)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        os.close(master)


def upgrade_metadata_deadlines(source, image, temp, env):
    api = container_for(source, "api")
    for index, suffix in enumerate((("ps", "-q", "api"), ("inspect", api, "--format", "{{.Image}}"))):
        marker = temp / f"upgrade-metadata-{index}"
        wrapped = dict(env, OPS_TEST_STALL_SUFFIX=json.dumps(suffix), OPS_TEST_STALL_MARKER=str(marker))
        backup = temp / f"never-created-upgrade-{index}"
        start = time.monotonic()
        result = project("upgrade", source, image, backup, success=False, env=wrapped, timeout=24)
        assert marker.exists() and 12 < time.monotonic() - start < 23
        assert "deadline" in result.stderr.lower() and not backup.exists()
        project("compose", source, "ps", timeout=12)


def restore_interruption(source, backup, image, temp, env, signum=None):
    suffix = "metadata" if signum is None else str(signum)
    name = f"beweft-ops-{RUN_ID}-restore-{suffix}"
    directory = temp / ("restore-interrupted-" + suffix)
    project("setup", directory, name, str(free_port()))
    project("image", directory, image)
    set_values(directory, {"BEWEFT_ENCRYPTION_KEY": settings(source)["BEWEFT_ENCRYPTION_KEY"]})
    try:
        marker = temp / ("restore-marker-" + suffix)
        if signum is None:
            wrapped = dict(env, OPS_TEST_STALL_SUFFIX=json.dumps(["volume", "inspect", name + "_media"]),
                           OPS_TEST_STALL_MARKER=str(marker))
            process = subprocess.Popen([ROOT / "project", "restore", directory, backup], cwd=ROOT, env=wrapped,
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
            try:
                for _ in range(1200):
                    if marker.exists():
                        break
                    assert process.poll() is None, "Restore exited before its volume lookup stalled"
                    time.sleep(0.1)
                else:
                    raise AssertionError("Restore volume lookup did not stall")
                started = time.monotonic()
                _out, stderr = process.communicate(timeout=24)
                assert process.returncode != 0 and b"deadline" in stderr.lower() and time.monotonic() - started < 23
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
            project("compose", directory, "ps", timeout=12)
            return
        large = temp / ("restore-large-" + suffix)
        shutil.copytree(backup, large)
        with tarfile.open(large / "media.tar", "w", format=tarfile.USTAR_FORMAT) as archive:
            entry = tarfile.TarInfo("bounded-test-file")
            entry.size = 8 * 1024 * 1024
            archive.addfile(entry, io.BytesIO(b"x" * entry.size))
        checksum(large)
        wrapped = dict(env, OPS_TEST_PASSTHROUGH="restore", OPS_TEST_PASSTHROUGH_MARKER=str(marker))
        process = subprocess.Popen([ROOT / "project", "restore", directory, large], cwd=ROOT, env=wrapped,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        try:
            for _ in range(600):
                if Path(str(marker) + ".pid").exists():
                    break
                assert process.poll() is None, "Restore exited before its stream helper started"
                time.sleep(0.1)
            else:
                raise AssertionError("Restore stream helper did not start")
            helper = json.loads(Path(str(marker) + ".args").read_text())[-1]
            docker("container", "inspect", helper)
            assert docker("inspect", "--format", "{{.State.Running}}", helper).stdout.strip() == "true"
            time.sleep(0.5)  # Fill the attach pipe while the plugin is not reading.
            if signum == signal.SIGINT:
                os.killpg(process.pid, signum)
            else:
                process.send_signal(signum)
            _stdout, stderr = process.communicate(timeout=25)
            assert process.returncode != 0 and b"daemon" in stderr.lower()
            docker("container", "inspect", helper, success=False)
            time.sleep(6.5)
            assert not marker.exists(), "A canceled restore plugin performed a delayed start"
            assert Path(str(marker) + ".signal").read_text() == str(signum), "The restore plugin did not receive cancellation"
            child = int(Path(str(marker) + ".pid").read_text())
            try:
                os.kill(child, 0)
            except ProcessLookupError:
                pass
            else:
                raise AssertionError("Canceled restore plugin remained alive")
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
    finally:
        remove_fixture_helpers(directory, env, name)
        project("compose", directory, "down", "--volumes", "--remove-orphans", timeout=120)
        no_resources(name)
        project("release", directory, timeout=30)


def restore_helper_records(env, project_name):
    path = Path(env["OPS_TEST_RESTORE_LOG"])
    if not path.exists():
        return []
    records = []
    for line in path.read_text().splitlines():
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue  # A fixture process may still be appending the last line.
        if record["name"].startswith(project_name + "-restore-"):
            records.append(record)
    return records


def check_restore_helper(record, directory, *, removed):
    name = record["name"]
    values = settings(directory)
    labels = record["labels"]
    expected_role = "restore-media-initialize" if "-restore-init-" in name else "restore-media-extract"
    assert labels["org.beweft.installation"] == values["BEWEFT_INSTALLATION_ID"]
    assert labels["org.beweft.operation"] == expected_role and labels["org.beweft.operation-id"] == name
    volume = values["COMPOSE_PROJECT_NAME"] + "_media"
    mounted = [mount for mount in record["mounts"] if mount.get("Type") == "volume"]
    assert any(mount.get("Name") == volume and mount.get("Destination") == "/media" for mount in mounted)
    assert any(mount.get("Target") == "/media" and mount.get("VolumeOptions", {}).get("NoCopy") is True
               for mount in record["configured"])
    if removed:
        docker("container", "inspect", name, success=False)
        for mount in mounted:
            if mount["Name"] != volume:
                assert re.fullmatch(r"[a-f0-9]{64}", mount["Name"])
                docker("volume", "inspect", mount["Name"], success=False)
    docker("volume", "inspect", volume)  # Removing a helper must preserve this named volume.


def remove_fixture_helpers(directory, env, project_name):
    values = settings(directory)
    for record in restore_helper_records(env, project_name):
        name = record["name"]
        inspected = subprocess.run(["docker", "container", "inspect", "--format", "{{json .Config.Labels}}", name],
                                   env=CLEAN_ENV, capture_output=True, text=True, timeout=15)
        if inspected.returncode:
            continue
        labels = json.loads(inspected.stdout)
        assert labels.get("org.beweft.installation") == values["BEWEFT_INSTALLATION_ID"]
        assert labels.get("org.beweft.operation-id") == name
        docker("rm", "-fv", name)


def volume_fixture(image, volume, *command):
    name = f"beweft-ops-{RUN_ID}-volume-check-{uuid.uuid4().hex[:8]}"
    try:
        return docker("run", "--rm", "--name", name, "--network", "none", "--user", "0:0", "--mount",
                      f"type=volume,src={volume},dst=/media,volume-nocopy", *command)
    finally:
        subprocess.run(["docker", "rm", "-fv", name], env=CLEAN_ENV, capture_output=True, timeout=15)


def media_volume_status(directory, image):
    volume = settings(directory)["COMPOSE_PROJECT_NAME"] + "_media"
    result = volume_fixture(image, volume, "--read-only", "--entrypoint", "sh", image,
                            "-c", 'stat -c "%u:%g" /media; ls -A /media')
    return result.stdout.strip().splitlines()


def initializer_interruption(source, backup, image, temp, env, mode, signum):
    suffix = mode.replace("-", "") + str(signum)
    name = f"beweft-ops-{RUN_ID}-init-{suffix}"
    directory = temp / ("initializer-interrupt-" + suffix)
    project("setup", directory, name, str(free_port()))
    project("image", directory, image)
    set_values(directory, {"BEWEFT_ENCRYPTION_KEY": settings(source)["BEWEFT_ENCRYPTION_KEY"]})
    marker = temp / ("initializer-marker-" + suffix)
    wrapped = dict(env, OPS_TEST_INITIALIZER_MODE=mode, OPS_TEST_INITIALIZER_MARKER=str(marker))
    process = subprocess.Popen([ROOT / "project", "restore", directory, backup], cwd=ROOT, env=wrapped,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
    try:
        record = None
        for _ in range(900):
            records = [item for item in restore_helper_records(env, name) if "-restore-init-" in item["name"]]
            if records and (not mode.startswith("delay-create") or marker.exists()):
                record = records[0]
                if mode.startswith("delay-create") or docker("inspect", "--format", "{{.State.Running}}", record["name"]).stdout.strip() == "true":
                    break
            assert process.poll() is None, "Restore exited before the owned initializer reached the intended phase"
            time.sleep(0.1)
        else:
            raise AssertionError("Owned initializer did not reach the intended phase")
        check_restore_helper(record, directory, removed=False)
        assert media_volume_status(directory, image) == ["0:0"]
        if signum == signal.SIGINT:
            os.killpg(process.pid, signum)
        else:
            process.send_signal(signum)
        _out, stderr = process.communicate(timeout=60)
        assert process.returncode != 0 and record["name"].encode() in stderr and b"daemon" in stderr.lower()
        check_restore_helper(record, directory, removed=True)
        if mode == "delay-start":
            time.sleep(10.5)
        assert media_volume_status(directory, image) == ["0:0"], "A canceled initializer changed the target later"
    finally:
        if process.poll() is None:
            process.send_signal(signal.SIGTERM)
            try:
                process.wait(timeout=60)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        remove_fixture_helpers(directory, env, name)
        project("compose", directory, "down", "--volumes", "--remove-orphans", timeout=120)
        no_resources(name)
        project("release", directory, timeout=30)


def initializer_failure(source, backup, image, temp, env, mode):
    name = f"beweft-ops-{RUN_ID}-init-{mode.replace('-', '')}"
    directory = temp / ("initializer-failure-" + mode)
    project("setup", directory, name, str(free_port()))
    project("image", directory, image)
    set_values(directory, {"BEWEFT_ENCRYPTION_KEY": settings(source)["BEWEFT_ENCRYPTION_KEY"]})
    wrapped = dict(env, OPS_TEST_INITIALIZER_MODE=mode)
    record = None
    try:
        result = project("restore", directory, backup, success=False, env=wrapped, timeout=180)
        records = [item for item in restore_helper_records(env, name) if "-restore-init-" in item["name"]]
        assert len(records) == 1
        record = records[0]
        assert record["name"] in result.stderr
        assert ("initializer" if mode == "start-fail" else "cleanup") in result.stderr.lower()
        check_restore_helper(record, directory, removed=mode == "start-fail")
        assert len(restore_helper_records(env, name)) == 1, "Extraction must not start after initializer failure"
    finally:
        if record is not None and mode == "remove-fail":
            # Only this recorded, label-verified helper belongs to the fixture.
            check_restore_helper(record, directory, removed=False)
            docker("rm", "-fv", record["name"])
            check_restore_helper(record, directory, removed=True)
        remove_fixture_helpers(directory, env, name)
        project("compose", directory, "down", "--volumes", "--remove-orphans", timeout=120)
        no_resources(name)
        project("release", directory, timeout=30)


def nonempty_initializer_refusal(image, temp, env):
    name = f"beweft-ops-{RUN_ID}-init-nonempty"
    directory = temp / "initializer-nonempty"
    project("setup", directory, name, str(free_port()))
    project("image", directory, image)
    values = settings(directory)
    volume = name + "_media"
    docker("volume", "create", "--label", "com.docker.compose.project=" + name,
           "--label", "com.docker.compose.volume=media", "--label", "org.beweft.installation=" + values["BEWEFT_INSTALLATION_ID"], volume)
    try:
        volume_fixture(image, volume, "--entrypoint", "sh", image, "-c", "printf fixture-content > /media/keep")
        code = '''import runpy,sys
m=runpy.run_path(sys.argv[1]); i=m["Installation"](sys.argv[2])
with i.locked():
    m["initialize_media"](i, "type=volume,src="+sys.argv[3]+",dst=/media,volume-nocopy", i.values["BEWEFT_IMAGE"])
'''
        result = run(["python3", "-c", code, ROOT / "scripts/self_host.py", directory, volume], success=False, env=env, timeout=60)
        records = restore_helper_records(env, name)
        assert len(records) == 1 and records[0]["name"] in result.stderr
        check_restore_helper(records[0], directory, removed=True)
        assert media_volume_status(directory, image) == ["0:0", "keep"]
        contents = volume_fixture(image, volume, "--entrypoint", "cat", image, "/media/keep").stdout
        assert contents == "fixture-content"
    finally:
        remove_fixture_helpers(directory, env, name)
        docker("volume", "rm", volume)
        no_resources(name)
        project("release", directory, timeout=30)


def helper_ownership_refusal(image):
    name = f"beweft-ops-{RUN_ID}-foreign-helper"
    docker("create", "--name", name, "--label", "org.beweft.installation=other",
           "--label", "org.beweft.operation=restore-media-initialize", "--label", "org.beweft.operation-id=" + name,
           "--network", "none", "--entrypoint", "sh", image, "-c", "exit 0")
    try:
        code = '''import runpy,sys
m=runpy.run_path(sys.argv[1]); name=sys.argv[2]
m["remove_named_helper"](name, {"org.beweft.installation":"expected", "org.beweft.operation":"restore-media-initialize", "org.beweft.operation-id":name})
'''
        result = run(["python3", "-c", code, ROOT / "scripts/self_host.py", name], success=False, timeout=45)
        assert "unexpected ownership" in result.stderr
        docker("container", "inspect", name)  # A foreign or malformed helper is not adopted or removed.
    finally:
        docker("rm", "-fv", name)


def restore_empty_media(source, backup, image, temp, env, brand):
    name = f"beweft-ops-{RUN_ID}-restore-empty"
    directory = temp / "restore-empty"
    port = free_port()
    project("setup", directory, name, str(port))
    project("image", directory, image)
    set_values(directory, {"BEWEFT_ENCRYPTION_KEY": settings(source)["BEWEFT_ENCRYPTION_KEY"]})
    try:
        project("restore", directory, backup, env=env, timeout=480)
        assert settings(directory)["BEWEFT_WORKER_ACTIVE"] == "false"
        records = restore_helper_records(env, name)
        assert len(records) == 2
        for record in records:
            check_restore_helper(record, directory, removed=True)
        assert media_volume_status(directory, image) == ["10001:10001"]
        client = Client(port)
        client.login()
        assert client.call("/media?brand_id=" + brand["id"]) == []
        boundary = "beweft-empty-restore"
        body = (f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="pixel.png"\r\nContent-Type: image/png\r\n\r\n'.encode() + PNG + f"\r\n--{boundary}--\r\n".encode())
        media = client.call("/media?brand_id=" + brand["id"], raw=body, content_type="multipart/form-data; boundary=" + boundary)
        assert client.media(media["url"]) == PNG
    finally:
        remove_fixture_helpers(directory, env, name)
        project("compose", directory, "down", "--volumes", "--remove-orphans", timeout=120)
        no_resources(name)
        project("release", directory, timeout=30)


def release_metadata_deadline(temp, env):
    """A stalled empty-project listing must fail without deleting its reservation."""
    name = f"beweft-ops-{RUN_ID}-release"
    install = temp / "release-deadline"
    marker = temp / "release-list-stall"
    project("setup", install, name, str(free_port()), "--worker-held")
    try:
        started = time.monotonic()
        result = project("release", install, success=False, env=dict(env, OPS_TEST_STALL_LIST=str(marker)), timeout=25)
        assert marker.exists() and "deadline" in result.stderr.lower()
        assert time.monotonic() - started < 23
        assert docker("volume", "inspect", "beweft-reservation-" + name).returncode == 0
    finally:
        project("release", install)


def metadata_stall(source, temp, env, *, interrupt, after_stop=False):
    api = container_for(source, "api")
    started = started_at(api)
    stem = ("state-" if after_stop else "metadata-") + ("signal" if interrupt else "timeout")
    marker = temp / stem
    destination = temp / (stem + "-backup")
    wrapped = dict(env, OPS_TEST_STALL=str(marker))
    if after_stop:
        wrapped["OPS_TEST_STATE_TARGET"] = api
    process = subprocess.Popen([str(ROOT / "project"), "backup", str(source), str(destination), "--max-downtime", "5"],
                               cwd=ROOT, env=wrapped, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, text=True, start_new_session=True)
    try:
        for _ in range(120):
            if marker.exists():
                break
            assert process.poll() is None, "Backup exited before the selected metadata call stalled"
            time.sleep(0.05)
        else:
            raise AssertionError("The intended Docker metadata call did not stall")
        begin = time.monotonic()
        if interrupt:
            process.send_signal(signal.SIGTERM)
        _, stderr = process.communicate(timeout=40 if after_stop else 8)
        assert process.returncode != 0 and time.monotonic() - begin < (35 if after_stop else 7)
        assert ("interrupted" if interrupt else "downtime limit expired") in stderr.lower()
        assert (started_at(api) != started) == after_stop
        assert docker("inspect", "--format", "{{.State.Running}}", api).stdout.strip() == "true"
        Client(int(settings(source)["BEWEFT_PORT"])).call("/auth/status")
        assert destination.exists() == after_stop
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=5)


def ownership_cases(source, temp):
    """Competing directories must never reconfigure a reserved project."""
    name = settings(source)["COMPOSE_PROJECT_NAME"]
    other_name = f"beweft-ops-{RUN_ID}-other"
    other = temp / "ownership-other"
    project("setup", other, other_name, str(free_port()))
    original_api = container_for(source, "api")
    original_db = container_for(source, "db")
    owner = settings(source)["BEWEFT_INSTALLATION_ID"]
    try:
        project("release", source, success=False)
        for container in (original_api, original_db, container_for(source, "worker")):
            assert docker("container", "inspect", "--format", '{{index .Config.Labels "org.beweft.installation"}}',
                          container).stdout.strip() == owner
        for kind, resource in (("network", name + "_default"), ("volume", name + "_database"), ("volume", name + "_media")):
            assert docker(kind, "inspect", "--format", '{{index .Labels "org.beweft.installation"}}',
                          resource).stdout.strip() == owner
        copied = temp / "copied-installation"
        copied.mkdir()
        shutil.copy2(source / "installation.env", copied / "installation.env")
        shutil.copy2(source / "installation-state.json", copied / "installation-state.json")
        project("compose", copied, "up", "-d", success=False)
        for override in (("-p", other_name), ("--project-name", other_name),
                         ("--project-name=" + other_name,), ("--env-file", str(other / "installation.env")),
                         ("-f", str(ROOT / "compose.yaml"))):
            project("compose", source, *override, "up", "-d", success=False)
        for override in (("up", "--build"), ("up", "--pull=always"), ("up", "--env", "BEWEFT_WORKER_ACTIVE=true"),
                         ("up", "--entrypoint", "sh"), ("down", "--rmi", "all")):
            project("compose", source, *override, success=False)
        # Simulate a second pre-existing configuration targeting the same
        # project, both sequentially and at the same time as its real owner.
        project("image", other, settings(source)["BEWEFT_IMAGE"])
        set_values(other, {"COMPOSE_PROJECT_NAME": name})
        project("compose", other, "up", "-d", "--no-build", "--pull", "never", success=False)
        command = ("up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never")
        jobs = [subprocess.Popen([str(ROOT / "project"), "compose", str(directory), *command], cwd=ROOT,
                                 env=CLEAN_ENV, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                for directory in (source, other)]
        try:
            results = [job.communicate(timeout=150) for job in jobs]
            assert jobs[0].returncode == 0 and jobs[1].returncode != 0
            assert "reservation" in results[1][1].lower()
        finally:
            for job in jobs:
                if job.poll() is None:
                    job.kill()
                    job.wait(timeout=5)
        assert container_for(source, "api") == original_api and container_for(source, "db") == original_db
    finally:
        set_values(other, {"COMPOSE_PROJECT_NAME": other_name})
        project("release", other)
    race_name = f"beweft-ops-{RUN_ID}-race"
    contenders = [temp / "race-a", temp / "race-b"]
    ports = [free_port(), free_port()]
    jobs = [subprocess.Popen([str(ROOT / "project"), "setup", str(path), race_name, str(port)], cwd=ROOT,
                             env=CLEAN_ENV, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            for path, port in zip(contenders, ports)]
    try:
        for job in jobs:
            job.communicate(timeout=45)
        assert sorted(job.returncode for job in jobs) == [0, 1], "Two simultaneous setups claimed the same project"
        winner = contenders[next(index for index, job in enumerate(jobs) if job.returncode == 0)]
        assert sum((path / "installation.env").exists() for path in contenders) == 1
        project("release", winner)
    finally:
        for job in jobs:
            if job.poll() is None:
                job.kill()
                job.wait(timeout=5)
        # The name is unique to this rehearsal. Only remove a reservation
        # if its owner is the configuration generated above.
        for path in contenders:
            if (path / "installation.env").exists():
                subprocess.run([str(ROOT / "project"), "release", str(path)], cwd=ROOT, env=CLEAN_ENV,
                               capture_output=True, timeout=20)


def guard_fails_closed():
    for value in (None, "invalid"):
        args = ["run", "--detach", "--rm", "--network", "none", "--name", HOLD_CONTAINER,
                "--env", "BEWEFT_ENFORCE_WORKER_HOLD=true"]
        if value is not None:
            args += ["--env", "BEWEFT_WORKER_ACTIVE=" + value]
        try:
            docker(*args, IMAGE, "beweft-worker")
            assert docker("inspect", "--format", "{{.State.Running}}", HOLD_CONTAINER).stdout.strip() == "true"
            docker("exec", HOLD_CONTAINER, "container-entrypoint", "beweft-worker", "--healthcheck", success=False)
            assert "beweft-worker" not in docker("top", HOLD_CONTAINER, "-eo", "pid,args").stdout
        finally:
            subprocess.run(["docker", "rm", "-f", HOLD_CONTAINER], env=CLEAN_ENV, capture_output=True, timeout=15)


def held_upload_timeout(source, client, brand, token, destination):
    """Keep a real authenticated upload open through a short backup deadline."""
    api = container_for(source, "api")
    old_started = started_at(api)
    db = container_for(source, "db")
    before = client.call("/media?brand_id=" + brand["id"])
    connection = http.client.HTTPConnection("127.0.0.1", int(settings(source)["BEWEFT_PORT"]), timeout=15)
    boundary = "beweft-held-upload"
    try:
        connection.putrequest("POST", "/api/v1/media?brand_id=" + brand["id"])
        connection.putheader("Authorization", "Bearer " + token)
        connection.putheader("Content-Type", "multipart/form-data; boundary=" + boundary)
        connection.putheader("Content-Length", str(8 * 1024 * 1024))
        connection.endheaders()
        connection.send((f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="held.png"\r\n'
                         'Content-Type: image/png\r\n\r\n').encode() + PNG[:24])
        for _ in range(100):
            count = docker("exec", db, "psql", "-qAt", "-U", "beweft", "-d", "beweft", "-c",
                           "SELECT count(*) FROM media_upload_reservations").stdout.strip()
            if count != "0":
                break
            time.sleep(0.1)
        else:
            raise AssertionError("The authenticated upload did not reserve its fixture")
        result = project("backup", source, destination, "--max-downtime", "3", success=False, timeout=90)
        assert "downtime limit expired" in result.stderr
    finally:
        connection.close()
    assert started_at(api) != old_started
    time.sleep(5)  # A detached late stop must not take down the restarted API.
    assert docker("inspect", "--format", "{{.State.Running}}", api).stdout.strip() == "true"
    client.call("/auth/status")
    assert client.call("/media?brand_id=" + brand["id"]) == before


def interrupted_dump(source, destination, stop_signal=signal.SIGTERM, env=None):
    """Interrupt a backup whose uniquely owned pg_dump is blocked on a DB lock."""
    db = container_for(source, "db")
    api = container_for(source, "api")
    old_started = started_at(api)
    name = "beweft-ops-sigterm-lock-" + RUN_ID
    locker = subprocess.Popen(["docker", "exec", "-e", "PGAPPNAME=" + name, db, "psql", "-qAt", "-U", "beweft", "-d", "beweft", "-c",
                               "BEGIN; LOCK TABLE owners IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(90); COMMIT;"],
                              env=CLEAN_ENV, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    process = None
    try:
        query = ("SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid "
                 "WHERE a.application_name='" + name + "' AND l.relation='owners'::regclass "
                 "AND l.mode='AccessExclusiveLock' AND l.granted")
        for _ in range(100):
            if docker("exec", db, "psql", "-qAt", "-U", "beweft", "-d", "beweft", "-c", query).stdout.strip() == "1":
                break
            time.sleep(0.1)
        else:
            raise AssertionError("SIGTERM fixture did not acquire its own table lock")
        process = subprocess.Popen([str(ROOT / "project"), "backup", str(source), str(destination), "--max-downtime", "120"],
                                   cwd=ROOT, env=CLEAN_ENV if env is None else env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                   start_new_session=True)
        waiting = "SELECT count(*) FROM pg_stat_activity WHERE application_name LIKE 'beweft-backup-%' AND wait_event_type='Lock'"
        for _ in range(200):
            assert process.poll() is None, "Backup ended before its dump blocked"
            if docker("exec", db, "psql", "-qAt", "-U", "beweft", "-d", "beweft", "-c", waiting).stdout.strip() == "1":
                break
            time.sleep(0.1)
        else:
            raise AssertionError("Owned pg_dump did not block on the fixture lock")
        if stop_signal == signal.SIGINT:
            os.killpg(process.pid, stop_signal)  # Like Ctrl-C in a terminal.
        else:
            process.send_signal(stop_signal)
        _stdout, stderr = process.communicate(timeout=90)
        assert process.returncode != 0 and "interrupted" in stderr.lower()
        # The unrelated lock is still held while the owned dump has gone.
        assert docker("exec", db, "psql", "-qAt", "-U", "beweft", "-d", "beweft", "-c", query).stdout.strip() == "1"
        assert docker("exec", db, "psql", "-qAt", "-U", "beweft", "-d", "beweft", "-c",
                      "SELECT count(*) FROM pg_stat_activity WHERE application_name LIKE 'beweft-backup-%'").stdout.strip() == "0"
        assert not docker("ps", "-aq", "--filter", "name=" + PROJECTS[0] + "-dump-").stdout.strip()
        assert started_at(api) != old_started
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            process.wait(timeout=30)
        sql = "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name='" + name + "' AND datname=current_database()"
        docker("exec", db, "psql", "-qAt", "-U", "beweft", "-d", "beweft", "-c", sql)
        locker.wait(timeout=20)
    client = Client(int(settings(source)["BEWEFT_PORT"]))
    client.call("/auth/status")


def backup_deadline(source, destination, env=None):
    """Block pg_dump on this fixture's owners table and verify recovery."""
    container = project("compose", source, "ps", "-q", "db").stdout.strip()
    name = "beweft-ops-lock-" + RUN_ID
    locker = subprocess.Popen(["docker", "exec", "-e", "PGAPPNAME=" + name, container, "psql", "-qAt",
                               "-U", "beweft", "-d", "beweft", "-c",
                               "BEGIN; LOCK TABLE owners IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(15); COMMIT;"],
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=CLEAN_ENV)
    try:
        query = ("SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid "
                 "WHERE a.application_name='" + name + "' AND l.relation='owners'::regclass "
                 "AND l.mode='AccessExclusiveLock' AND l.granted")
        for _ in range(50):
            ready = docker("exec", container, "psql", "-qAt", "-U", "beweft", "-d", "beweft", "-c", query).stdout.strip()
            if ready == "1":
                break
            time.sleep(0.1)
        else:
            raise AssertionError("Backup deadline fixture did not acquire its own table lock")
        started = time.monotonic()
        result = project("backup", source, destination, "--max-downtime", "4", success=False, timeout=45, env=env)
        assert "downtime limit expired" in result.stderr
        assert time.monotonic() - started < 35
    finally:
        assert locker.wait(timeout=20) == 0, "Backup deadline fixture did not release its lock"
    for _ in range(50):
        remaining = docker("exec", container, "psql", "-qAt", "-U", "beweft", "-d", "beweft", "-c",
                           "SELECT count(*) FROM pg_stat_activity WHERE application_name LIKE 'beweft-backup-%'").stdout.strip()
        if remaining == "0":
            break
        time.sleep(0.1)
    else:
        raise AssertionError("Timed-out dump process remained in the fixture database")
    project("compose", source, "up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never")


def main(temp):
    global BUILT_IMAGE
    source, restored, rollback = [temp / value for value in ("source", "restored", "rollback")]
    image_fixture, build_fixture = temp / "image-fixture", temp / "build-fixture"
    source_backup, upgrade_backup = temp / "backup", temp / "upgrade-backup"
    ports = [free_port() for _ in PROJECTS]
    assert len(set(ports)) == len(ports)
    original_image = docker("image", "inspect", "--format", "{{.Id}}", IMAGE).stdout.strip()
    postgres_image = docker("image", "inspect", "--format", "{{.Id}}", "postgres:17-alpine").stdout.strip()
    docker("image", "tag", postgres_image, POSTGRES_ALIAS)
    assert "beweft <tool>" in docker("run", "--rm", "--network", "none", "--entrypoint", "beweft", IMAGE, "--help").stdout
    assert "ffprobe version" in docker("run", "--rm", "--network", "none", "--entrypoint", "ffprobe", IMAGE, "-version").stdout
    guard_fails_closed()
    # Re-tagging a local alias for another installation must not change the
    # immutable ID stored by the first one.
    docker("image", "tag", IMAGE, MUTABLE_IMAGE)
    project("setup", source, PROJECTS[0], str(ports[0]), "--postgres-image", POSTGRES_ALIAS)
    project("setup", temp / "same-project", PROJECTS[0], str(free_port()), success=False)
    assert settings(source)["BEWEFT_POSTGRES_IMAGE"] == postgres_image
    # Change only a unique test alias; the first installation and its dump
    # client must keep the original database image even after that retag.
    docker("create", "--name", POSTGRES_BUILDER, "--network", "none", postgres_image)
    pg_mounts = json.loads(docker("inspect", "--format", "{{json .Mounts}}", POSTGRES_BUILDER).stdout)
    try:
        docker("commit", "--change", "LABEL beweft.rehearsal=postgres", POSTGRES_BUILDER, POSTGRES_DERIVED)
    finally:
        docker("rm", "-fv", POSTGRES_BUILDER)
    for mount in pg_mounts:
        if mount.get("Type") == "volume":
            docker("volume", "inspect", mount["Name"], success=False)
    changed_postgres = docker("image", "inspect", "--format", "{{.Id}}", POSTGRES_DERIVED).stdout.strip()
    assert changed_postgres != postgres_image
    docker("image", "tag", POSTGRES_DERIVED, POSTGRES_ALIAS)
    project("compose", source, "up", "-d", success=False)
    no_resources(PROJECTS[0])
    project("image", source, MUTABLE_IMAGE)
    assert settings(source)["BEWEFT_IMAGE"] == original_image
    docker("create", "--name", BUILDER, "--network", "none", IMAGE)
    try:
        docker("commit", "--change", "LABEL beweft.rehearsal=upgrade", BUILDER, UPGRADE_IMAGE)
    finally:
        docker("rm", BUILDER)
    upgraded_image = docker("image", "inspect", "--format", "{{.Id}}", UPGRADE_IMAGE).stdout.strip()
    assert upgraded_image != original_image
    for field in ("Entrypoint", "Cmd"):
        template = "{{json .Config." + field + "}}"
        assert docker("image", "inspect", "--format", template, UPGRADE_IMAGE).stdout == \
            docker("image", "inspect", "--format", template, IMAGE).stdout
    for directory, name, port in zip((restored, rollback), PROJECTS[1:], ports[1:]):
        project("setup", directory, name, str(port))
        project("image", directory, IMAGE)
        settings(directory)
    project("setup", source, PROJECTS[0], str(ports[0]), success=False)
    assert (source / "installation.env").stat().st_mode & 0o077 == 0
    print("Rehearsal: starting isolated installation", flush=True)
    project("compose", source, "up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never")
    assert image_for(source) == original_image
    assert image_for(source, "db") == postgres_image
    managed_state_refusal(source, upgraded_image)
    ownership_cases(source, temp)
    set_values(source, {"BEWEFT_POSTGRES_IMAGE": changed_postgres})
    try:
        project("compose", source, "up", "-d", "--no-build", "--pull", "never", success=False)
    finally:
        set_values(source, {"BEWEFT_POSTGRES_IMAGE": postgres_image})
    assert image_for(source, "db") == postgres_image
    original_container = container_for(source, "api")
    # A second installation's build uses a private tag and cannot retag the first.
    project("setup", build_fixture, f"beweft-ops-{RUN_ID}-build", str(free_port()))
    instrumented, helper_log = instrumented_docker(temp)
    passthrough_interruptions(source, build_fixture, temp, instrumented)
    upgrade_metadata_deadlines(source, UPGRADE_IMAGE, temp, instrumented)
    node_id = docker("image", "inspect", "--format", "{{.Id}}", "node:24-alpine").stdout.strip()
    built = project("build", build_fixture, "--node-image", "node@" + node_id, timeout=600)
    match = re.search(r"Built (beweft:[a-z0-9_-]+) and pinned", built.stdout)
    assert match and settings(build_fixture)["BEWEFT_IMAGE"].startswith("sha256:")
    BUILT_IMAGE = match.group(1)
    docker("image", "tag", UPGRADE_IMAGE, MUTABLE_IMAGE)
    project("setup", image_fixture, f"beweft-ops-{RUN_ID}-image", str(free_port()))
    project("image", image_fixture, MUTABLE_IMAGE)
    assert settings(image_fixture)["BEWEFT_IMAGE"] == upgraded_image
    project("compose", source, "up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never")
    assert settings(source)["BEWEFT_IMAGE"] == original_image and image_for(source) == original_image
    assert image_for(source, "db") == postgres_image
    assert container_for(source, "api") == original_container
    project("compose", source, "up", "-d", "--build", success=False)
    project("image", source, UPGRADE_IMAGE, success=False)
    client = Client(ports[0])
    with client.client.open(client.origin + "/", timeout=15) as response:
        assert b'<div id="root">' in response.read()
    client.call("/auth/bootstrap", {"setup_secret": settings(source)["BEWEFT_SETUP_SECRET"], "email": EMAIL, "password": PASSWORD})
    brand = client.call("/brands", {"name": "Local recovery fixture"})
    channel = client.call("/channels", {"brand_id": brand["id"], "name": "Manual fixture", "provider": "manual", "timezone": "UTC"})
    token = client.call("/tokens", {"name": "Scoped CLI fixture", "scopes": ["read"], "brand_ids": [brand["id"]]})["token"]
    SENSITIVE.add(token)
    upload_token = client.call("/tokens", {"name": "Held upload fixture", "scopes": ["drafts:write"], "brand_ids": [brand["id"]]})["token"]
    SENSITIVE.add(upload_token)
    cli = project("cli", source, "brands",
                  env=dict(CLEAN_ENV, BEWEFT_TOKEN=token))
    assert token not in cli.stdout + cli.stderr and brand["id"] in cli.stdout
    live_sessions_release_lock(source, token, temp / "live-session-backup")
    interactive_cli_terminal(source, token)
    boundary = "beweft-rehearsal-boundary"
    multipart = (f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="pixel.png"\r\nContent-Type: image/png\r\n\r\n'.encode() + PNG + f"\r\n--{boundary}--\r\n".encode())
    media = client.call("/media?brand_id=" + brand["id"], raw=multipart, content_type="multipart/form-data; boundary=" + boundary)
    post = client.call("/posts", {"brand_id": brand["id"], "text": "Private recovery draft", "media_ids": [media["id"]]})
    when = (datetime.now(timezone.utc) + timedelta(days=3)).isoformat()
    publication = client.call(f"/posts/{post['id']}/schedule", {"version": post["version"], "channel_ids": [channel["id"]], "scheduled_at": when})[0]
    assert publication["state"] == "queued"
    project("compose", source, "restart", "db", "api", "worker")
    project("compose", source, "up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never")
    verify_content(Client(ports[0]), brand, post, publication, media)
    release_metadata_deadline(temp, instrumented)
    cleanup_process_group(source, temp, instrumented, admitted=False)
    cleanup_process_group(source, temp, instrumented, admitted=True)
    cleanup_process_group(source, temp, instrumented, admitted=False, interrupted=signal.SIGINT)
    cleanup_process_group(source, temp, instrumented, admitted=False, interrupted=signal.SIGTERM)
    metadata_stall(source, temp, instrumented, interrupt=False)
    metadata_stall(source, temp, instrumented, interrupt=True)
    metadata_stall(source, temp, instrumented, interrupt=False, after_stop=True)
    metadata_stall(source, temp, instrumented, interrupt=True, after_stop=True)
    held_upload_timeout(source, client, brand, upload_token, temp / "upload-timeout-backup")
    interrupted_dump(source, temp / "interrupted-backup", env=instrumented)
    helper_offset = assert_helpers_removed(helper_log, 0, postgres_image)
    interrupted_dump(source, temp / "interrupt-group-backup", signal.SIGINT, env=instrumented)
    helper_offset = assert_helpers_removed(helper_log, helper_offset, postgres_image)

    hostile = dict(CLEAN_ENV, COMPOSE_PROJECT_NAME=PROJECTS[1], BEWEFT_IMAGE="wrong:wrong",
                   BEWEFT_DB_PASSWORD="wrong", BEWEFT_ENCRYPTION_KEY="wrong")
    print("Rehearsal: backup and refusal cases", flush=True)
    project("backup", source, source_backup, env=dict(instrumented, **{key: value for key, value in hostile.items()
                                                   if key.startswith(("COMPOSE_", "BEWEFT_"))}), timeout=480)
    helper_offset = assert_helpers_removed(helper_log, helper_offset, postgres_image)
    check_backup_media_helpers(instrumented, source)
    assert (source_backup / "postgres-image-id").read_text().strip() == postgres_image
    assert image_for(source) == original_image
    verify_content(Client(ports[0]), brand, post, publication, media)
    initializer_interruption(source, source_backup, IMAGE, temp, instrumented, "delay-start", signal.SIGINT)
    initializer_interruption(source, source_backup, IMAGE, temp, instrumented, "delay-start", signal.SIGTERM)
    initializer_interruption(source, source_backup, IMAGE, temp, instrumented, "delay-create", signal.SIGTERM)
    initializer_interruption(source, source_backup, IMAGE, temp, instrumented, "delay-create-overdue", signal.SIGTERM)
    initializer_failure(source, source_backup, IMAGE, temp, instrumented, "start-fail")
    initializer_failure(source, source_backup, IMAGE, temp, instrumented, "remove-fail")
    nonempty_initializer_refusal(IMAGE, temp, instrumented)
    helper_ownership_refusal(IMAGE)
    restore_empty_media(source, temp / "live-session-backup", IMAGE, temp, instrumented, brand)
    restore_interruption(source, source_backup, IMAGE, temp, instrumented)
    restore_interruption(source, source_backup, IMAGE, temp, instrumented, signal.SIGINT)
    restore_interruption(source, source_backup, IMAGE, temp, instrumented, signal.SIGTERM)
    project("backup", source, source_backup, success=False)
    backup_deadline(source, temp / "timed-out-backup", env=instrumented)
    helper_offset = assert_helpers_removed(helper_log, helper_offset, postgres_image)
    real_password = settings(source)["BEWEFT_DB_PASSWORD"]
    set_values(source, {"BEWEFT_DB_PASSWORD": "a" * 64})
    try:
        project("backup", source, temp / "failed-dump-backup", success=False, env=instrumented, timeout=90)
    finally:
        set_values(source, {"BEWEFT_DB_PASSWORD": real_password})
    helper_offset = assert_helpers_removed(helper_log, helper_offset, postgres_image)
    verify_content(Client(ports[0]), brand, post, publication, media)
    verify_content(Client(ports[0]), brand, post, publication, media)

    # The original installation can advance after the backup; its restored
    # queued copy must be reconciled before any worker is enabled.
    docker("exec", container_for(source, "db"), "psql", "-U", "beweft", "-d", "beweft", "-c",
            f"UPDATE publications SET scheduled_at=now()-interval '1 minute' WHERE id='{publication['id']}'")
    for _ in range(100):
        state = next(item for item in client.call("/publications") if item["id"] == publication["id"])["state"]
        if state == "manual_required":
            break
        time.sleep(0.2)
    else:
        raise AssertionError("Original fixture did not advance after backup")

    # The generated target key differs, so this fails before any target resources.
    project("restore", restored, source_backup, success=False, env=hostile)
    no_resources(PROJECTS[1])
    set_values(restored, {"BEWEFT_ENCRYPTION_KEY": settings(source)["BEWEFT_ENCRYPTION_KEY"]})
    wrong_db = temp / "wrong-database-image"
    wrong_db_project = f"beweft-ops-{RUN_ID}-wrongdb"
    project("setup", wrong_db, wrong_db_project, str(free_port()), "--postgres-image", changed_postgres)
    try:
        project("image", wrong_db, IMAGE)
        set_values(wrong_db, {"BEWEFT_ENCRYPTION_KEY": settings(source)["BEWEFT_ENCRYPTION_KEY"]})
        project("restore", wrong_db, source_backup, success=False)
        no_resources(wrong_db_project)
    finally:
        project("release", wrong_db)
    wrong_image = temp / "wrong-app-image"
    wrong_image_project = f"beweft-ops-{RUN_ID}-wrongapp"
    project("setup", wrong_image, wrong_image_project, str(free_port()))
    try:
        project("image", wrong_image, "alpine:3.22")
        set_values(wrong_image, {"BEWEFT_ENCRYPTION_KEY": settings(source)["BEWEFT_ENCRYPTION_KEY"]})
        project("restore", wrong_image, source_backup, success=False)
        no_resources(wrong_image_project)
    finally:
        project("release", wrong_image)
    sentinel = restored / "keep-me"
    sentinel.write_text("untouched")
    project("restore", restored, source_backup, success=False)
    assert sentinel.read_text() == "untouched"
    sentinel.unlink()
    no_resources(PROJECTS[1])
    for mode in ("checksum", "parent", "absolute", "link", "hardlink", "device", "duplicate", "pax"):
        malicious = temp / ("invalid-" + mode)
        malicious_backup(source_backup, malicious, mode)
        project("restore", restored, malicious, success=False)
        no_resources(PROJECTS[1])
    volume = PROJECTS[1] + "_media"
    docker("volume", "create", volume)
    try:
        project("restore", restored, source_backup, success=False)
        assert docker("volume", "inspect", volume).returncode == 0
    finally:
        docker("volume", "rm", volume)
    print("Rehearsal: restoring to empty project", flush=True)
    wrapped = dict(instrumented, **{key: value for key, value in hostile.items() if key.startswith(("COMPOSE_", "BEWEFT_"))})
    project("restore", restored, source_backup, env=wrapped, timeout=480)
    records = restore_helper_records(instrumented, PROJECTS[1])
    assert len(records) == 2
    for record in records:
        check_restore_helper(record, restored, removed=True)
    assert settings(restored)["BEWEFT_WORKER_ACTIVE"] == "false"
    project("compose", restored, "up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never")
    running = project("compose", restored, "ps", "--status", "running", "--services").stdout.splitlines()
    assert "api" in running and "db" in running and "worker" not in running
    verify_content(Client(ports[1]), brand, post, publication, media)
    # Even an overdue publication must remain untouched until the operator
    # explicitly starts the restored worker.
    docker("exec", container_for(restored, "db"), "psql", "-U", "beweft", "-d", "beweft", "-c",
            f"UPDATE publications SET scheduled_at=now()-interval '1 minute' WHERE id='{publication['id']}'")
    time.sleep(2)
    isolated = Client(ports[1])
    isolated.login()
    assert next(item for item in isolated.call("/publications") if item["id"] == publication["id"])["state"] == "queued"
    assert isolated.call(f"/publications/{publication['id']}/attempts") == []
    # Even explicit worker-profile selection and restart cannot bypass the hold.
    project("compose", restored, "--profile", "worker", "up", "-d", "--no-build", "--pull", "never", "worker")
    held = container_for(restored, "worker")
    docker("exec", held, "container-entrypoint", "beweft-worker", "--healthcheck", success=False)
    docker("restart", held)
    docker("exec", held, "container-entrypoint", "beweft-worker", "--healthcheck", success=False)
    restored_db = container_for(restored, "db")
    heartbeat_sql = "SELECT coalesce(string_agg(id || '|' || last_seen::text, ',' ORDER BY id), '') FROM worker_heartbeats"
    before_heartbeat = docker("exec", restored_db, "psql", "-qAt", "-U", "beweft", "-d", "beweft", "-c", heartbeat_sql).stdout
    for attack in (("run", "--rm", "-d", "--no-deps", "-e", "BEWEFT_WORKER_ACTIVE=true", "worker"),
                   ("run", "--rm", "--entrypoint", "beweft-worker", "worker"),
                   ("run", "--rm", "worker", "beweft-worker"),
                   ("exec", "-T", "worker", "beweft-worker"),
                   ("create", "worker")):
        project("compose", restored, "--profile", "worker", *attack, success=False)
    config = restored / "installation.env"
    original_config = config.read_text()
    try:
        config.write_text(original_config + "BEWEFT_WORKER_ACTIVE=true\n")
        config.chmod(0o600)
        project("compose", restored, "--profile", "worker", "up", "-d", "worker", success=False)
    finally:
        config.write_text(original_config)
        config.chmod(0o600)
    assert docker("exec", restored_db, "psql", "-qAt", "-U", "beweft", "-d", "beweft", "-c", heartbeat_sql).stdout == before_heartbeat
    commands = [line.split(None, 1)[1].split()[0] for line in
                docker("top", held, "-eo", "pid,args").stdout.splitlines()[1:] if len(line.split(None, 1)) == 2]
    assert all(Path(command).name != "beweft-worker" for command in commands)
    project("cli", restored, "--entrypoint", "beweft-worker", success=False)
    project("cli", restored, "-e", "BEWEFT_WORKER_ACTIVE=true", "worker", success=False)
    cli = project("cli", restored, "brands", env=dict(CLEAN_ENV, BEWEFT_TOKEN=token))
    assert brand["id"] in cli.stdout and token not in cli.stdout + cli.stderr
    assert docker("exec", restored_db, "psql", "-qAt", "-U", "beweft", "-d", "beweft", "-c", heartbeat_sql).stdout == before_heartbeat
    time.sleep(2)
    assert next(item for item in isolated.call("/publications") if item["id"] == publication["id"])["state"] == "queued"
    assert isolated.call(f"/publications/{publication['id']}/attempts") == []
    project("activate", restored, success=False)
    canceled = isolated.call(f"/publications/{publication['id']}/cancel", {})
    assert canceled["state"] == "canceled"
    project("compose", source, "stop", "worker")
    project("activate", restored, "--handover-reviewed")
    assert settings(restored)["BEWEFT_WORKER_ACTIVE"] == "true"
    assert next(item for item in isolated.call("/publications") if item["id"] == publication["id"])["state"] == "canceled"
    future_post = isolated.call("/posts", {"brand_id": brand["id"], "text": "Private recovery draft", "media_ids": [media["id"]]})
    future_pub = isolated.call(f"/posts/{future_post['id']}/schedule", {"version": future_post["version"], "channel_ids": [channel["id"]], "scheduled_at": when})[0]

    # The derived image differs by a label only: this exercises the complete
    # upgrade/rollback mechanics, not an application schema version change.
    print("Rehearsal: upgrade and rollback", flush=True)
    project("upgrade", restored, UPGRADE_IMAGE, upgrade_backup, timeout=600)
    assert image_for(restored) == upgraded_image
    upgraded = Client(ports[1])
    verify_content(upgraded, brand, future_post, future_pub, media)
    after = upgraded.call("/posts", {"brand_id": brand["id"], "text": "Only after upgrade"})

    set_values(rollback, {"BEWEFT_ENCRYPTION_KEY": settings(source)["BEWEFT_ENCRYPTION_KEY"]})
    project("restore", rollback, upgrade_backup, timeout=480)
    assert image_for(rollback) == original_image
    running = project("compose", rollback, "ps", "--status", "running", "--services").stdout.splitlines()
    assert "api" in running and "db" in running and "worker" not in running
    rolled_back = Client(ports[2])
    verify_content(rolled_back, brand, future_post, future_pub, media)
    posts = rolled_back.call("/posts?brand_id=" + brand["id"])
    assert all(item["id"] != after["id"] for item in posts)
    print("PASS: isolated installation, owner login, restart, byte-identical media and queued post, backup, safe rejection, restore, image upgrade and rollback")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="beweft-ops-rehearsal-") as temporary:
        temp = Path(temporary)
        try:
            main(temp)
        finally:
            failures = []
            for directory, name in zip((temp / "source", temp / "restored", temp / "rollback"), PROJECTS):
                if (directory / "installation.env").exists():
                    try:
                        project("compose", directory, "down", "--volumes", "--remove-orphans", timeout=120)
                        no_resources(name)
                        project("release", directory, timeout=30)
                    except Exception:
                        failures.append(name)
            for directory in (temp / "image-fixture", temp / "build-fixture"):
                if (directory / "installation.env").exists():
                    try:
                        project("release", directory, timeout=30)
                    except Exception:
                        failures.append(directory.name)
            try:
                for name in (BUILDER, HOLD_CONTAINER, POSTGRES_BUILDER):
                    subprocess.run(["docker", "rm", "-fv", name], env=CLEAN_ENV, capture_output=True, timeout=15)
                    if subprocess.run(["docker", "container", "inspect", name], env=CLEAN_ENV, capture_output=True, timeout=10).returncode == 0:
                        failures.append(name)
                listing = subprocess.run(["docker", "image", "ls", "--filter", f"reference=beweft:beweft-ops-{RUN_ID}-build-*",
                                          "--format", "{{.Repository}}:{{.Tag}}"], env=CLEAN_ENV, capture_output=True, text=True, timeout=20)
                assert listing.returncode == 0
                owned_images = set(listing.stdout.splitlines()) | {image for image in (UPGRADE_IMAGE, MUTABLE_IMAGE, POSTGRES_ALIAS, POSTGRES_DERIVED, BUILT_IMAGE) if image}
                for image in owned_images:
                    if image:
                        subprocess.run(["docker", "image", "rm", image], env=CLEAN_ENV, capture_output=True, timeout=30)
                        if subprocess.run(["docker", "image", "inspect", image], env=CLEAN_ENV, capture_output=True, timeout=10).returncode == 0:
                            failures.append(image)
            except Exception:
                failures.append("derived image or fixture container")
            assert not failures, "Could not remove owned rehearsal resources: " + ", ".join(failures)
