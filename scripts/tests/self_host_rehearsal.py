#!/usr/bin/env python3
"""Run a disposable, offline-provider install/recovery/upgrade rehearsal.

Requires an already built BEWEFT_REHEARSAL_IMAGE and the postgres:17-alpine
image. Creates only uniquely named projects, volumes and one derived
image; all are removed in finally. Never contacts a social provider.
"""
import base64
from datetime import datetime, timedelta, timezone
import hashlib
import http.cookiejar
import io
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[2]
IMAGE = os.environ.get("BEWEFT_REHEARSAL_IMAGE", "beweft:ops-rehearsal-20261029")
CLEAN_ENV = {key: value for key, value in os.environ.items()
             if not key.startswith(("BEWEFT_", "COMPOSE_")) and key != "DATABASE_URL"}
RUN_ID = uuid.uuid4().hex[:12]
PROJECTS = [f"beweft-ops-{RUN_ID}-{letter}" for letter in ("a", "b", "c")]
UPGRADE_IMAGE = f"beweft:ops-upgrade-{RUN_ID}"
BUILDER = f"beweft-ops-image-{RUN_ID}"
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
    names = ("database.dump", "media.tar", "key.sha256", "image-id", "manifest")
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


def backup_deadline(source, destination):
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
        result = project("backup", source, destination, "--max-downtime", "4", success=False, timeout=45)
        assert "downtime limit expired" in result.stderr
        assert time.monotonic() - started < 35
    finally:
        assert locker.wait(timeout=20) == 0, "Backup deadline fixture did not release its lock"
    for _ in range(50):
        remaining = docker("exec", container, "psql", "-qAt", "-U", "beweft", "-d", "beweft", "-c",
                           "SELECT count(*) FROM pg_stat_activity WHERE application_name='pg_dump'").stdout.strip()
        if remaining == "0":
            break
        time.sleep(0.1)
    else:
        raise AssertionError("Timed-out dump process remained in the fixture database")
    project("compose", source, "up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never")


def main(temp):
    source, restored, rollback = [temp / value for value in ("source", "restored", "rollback")]
    source_backup, upgrade_backup = temp / "backup", temp / "upgrade-backup"
    ports = [free_port() for _ in PROJECTS]
    assert len(set(ports)) == len(ports)
    original_image = docker("image", "inspect", "--format", "{{.Id}}", IMAGE).stdout.strip()
    assert "beweft <tool>" in docker("run", "--rm", "--network", "none", "--entrypoint", "beweft", IMAGE, "--help").stdout
    assert "ffprobe version" in docker("run", "--rm", "--network", "none", "--entrypoint", "ffprobe", IMAGE, "-version").stdout
    for directory, name, port in zip((source, restored, rollback), PROJECTS, ports):
        project("setup", directory, name, str(port))
        set_values(directory, {"BEWEFT_IMAGE": original_image})
        settings(directory)
    project("setup", source, PROJECTS[0], str(ports[0]), success=False)
    assert (source / "installation.env").stat().st_mode & 0o077 == 0
    print("Rehearsal: starting isolated installation", flush=True)
    project("compose", source, "up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never")
    client = Client(ports[0])
    with client.client.open(client.origin + "/", timeout=15) as response:
        assert b'<div id="root">' in response.read()
    client.call("/auth/bootstrap", {"setup_secret": settings(source)["BEWEFT_SETUP_SECRET"], "email": EMAIL, "password": PASSWORD})
    brand = client.call("/brands", {"name": "Local recovery fixture"})
    channel = client.call("/channels", {"brand_id": brand["id"], "name": "Manual fixture", "provider": "manual", "timezone": "UTC"})
    token = client.call("/tokens", {"name": "Scoped CLI fixture", "scopes": ["read"], "brand_ids": [brand["id"]]})["token"]
    SENSITIVE.add(token)
    cli = project("compose", source, "--profile", "tools", "run", "--rm", "-T", "cli", "brands",
                  env=dict(CLEAN_ENV, BEWEFT_TOKEN=token))
    assert token not in cli.stdout + cli.stderr and brand["id"] in cli.stdout
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

    hostile = dict(CLEAN_ENV, COMPOSE_PROJECT_NAME=PROJECTS[1], BEWEFT_IMAGE="wrong:wrong",
                   BEWEFT_DB_PASSWORD="wrong", BEWEFT_ENCRYPTION_KEY="wrong")
    print("Rehearsal: backup and refusal cases", flush=True)
    project("backup", source, source_backup, env=hostile, timeout=480)
    assert image_for(source) == original_image
    verify_content(Client(ports[0]), brand, post, publication, media)
    project("backup", source, source_backup, success=False)
    backup_deadline(source, temp / "timed-out-backup")
    verify_content(Client(ports[0]), brand, post, publication, media)

    # The generated target key differs, so this fails before any target resources.
    project("restore", restored, source_backup, success=False, env=hostile)
    no_resources(PROJECTS[1])
    set_values(restored, {"BEWEFT_ENCRYPTION_KEY": settings(source)["BEWEFT_ENCRYPTION_KEY"]})
    set_values(restored, {"BEWEFT_IMAGE": "alpine:3.22"})
    project("restore", restored, source_backup, success=False)
    no_resources(PROJECTS[1])
    set_values(restored, {"BEWEFT_IMAGE": original_image})
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
    project("restore", restored, source_backup, env=hostile, timeout=480)
    running = project("compose", restored, "ps", "--status", "running", "--services").stdout.splitlines()
    assert "api" in running and "db" in running and "worker" not in running
    verify_content(Client(ports[1]), brand, post, publication, media)
    # Even an overdue publication must remain untouched until the operator
    # explicitly starts the restored worker.
    project("compose", restored, "exec", "-T", "db", "psql", "-U", "beweft", "-d", "beweft", "-c",
            f"UPDATE publications SET scheduled_at=now()-interval '1 minute' WHERE id='{publication['id']}'")
    time.sleep(2)
    isolated = Client(ports[1])
    isolated.login()
    assert next(item for item in isolated.call("/publications") if item["id"] == publication["id"])["state"] == "queued"
    assert isolated.call(f"/publications/{publication['id']}/attempts") == []
    project("compose", restored, "exec", "-T", "db", "psql", "-U", "beweft", "-d", "beweft", "-c",
            f"UPDATE publications SET scheduled_at='{when}'::timestamptz WHERE id='{publication['id']}'")
    project("compose", source, "stop", "worker")
    project("compose", restored, "up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never", "worker")

    # The derived image differs by a label only: this exercises the complete
    # upgrade/rollback mechanics, not an application schema version change.
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
    print("Rehearsal: upgrade and rollback", flush=True)
    project("upgrade", restored, UPGRADE_IMAGE, upgrade_backup, timeout=600)
    assert image_for(restored) == upgraded_image
    upgraded = Client(ports[1])
    verify_content(upgraded, brand, post, publication, media)
    after = upgraded.call("/posts", {"brand_id": brand["id"], "text": "Only after upgrade"})

    set_values(rollback, {"BEWEFT_ENCRYPTION_KEY": settings(source)["BEWEFT_ENCRYPTION_KEY"]})
    project("restore", rollback, upgrade_backup, timeout=480)
    assert image_for(rollback) == original_image
    running = project("compose", rollback, "ps", "--status", "running", "--services").stdout.splitlines()
    assert "api" in running and "db" in running and "worker" not in running
    rolled_back = Client(ports[2])
    verify_content(rolled_back, brand, post, publication, media)
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
                    except Exception:
                        failures.append(name)
            try:
                subprocess.run(["docker", "rm", "-f", BUILDER], env=CLEAN_ENV, capture_output=True, timeout=15)
                if subprocess.run(["docker", "container", "inspect", BUILDER], env=CLEAN_ENV, capture_output=True, timeout=10).returncode == 0:
                    failures.append(BUILDER)
                subprocess.run(["docker", "image", "rm", UPGRADE_IMAGE], env=CLEAN_ENV, capture_output=True, timeout=30)
                if subprocess.run(["docker", "image", "inspect", UPGRADE_IMAGE], env=CLEAN_ENV, capture_output=True, timeout=10).returncode == 0:
                    failures.append(UPGRADE_IMAGE)
            except Exception:
                failures.append("derived image or fixture container")
            assert not failures, "Could not remove owned rehearsal resources: " + ", ".join(failures)
