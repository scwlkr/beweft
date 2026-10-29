#!/usr/bin/env python3
"""Private, single-host Compose installation and recovery commands."""

import argparse
from contextlib import ExitStack, contextmanager
from datetime import datetime, timezone
import fcntl
import hashlib
import os
from pathlib import Path, PurePosixPath
import re
import secrets
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
COMPOSE = ROOT / "compose.yaml"
PROJECT_PATTERN = re.compile(r"[a-z0-9][a-z0-9_-]{0,50}\Z")
IMAGE_PATTERN = re.compile(r"[a-zA-Z0-9][a-zA-Z0-9._:/@-]{0,511}\Z")
HEX64 = re.compile(r"[a-fA-F0-9]{64}\Z")
IMAGE_ID = re.compile(r"sha256:[a-f0-9]{64}\Z")
BACKUP_FILES = ("database.dump", "media.tar", "key.sha256", "image-id", "manifest")
CLEANUP_SECONDS = 150


class OperationError(Exception):
    pass


class StopUncertain(OperationError):
    """Do not restart if an earlier stop request might still take effect."""


class Cancelled(OperationError):
    pass


class Cancellation:
    def __init__(self):
        self.signal = None

    def handle(self, signum, _frame):
        self.signal = self.signal or signum

    def check(self):
        if self.signal:
            raise Cancelled("Backup interrupted; recovering the original services.")


@contextmanager
def termination_guard():
    cancellation = Cancellation()
    previous = {signum: signal.signal(signum, cancellation.handle) for signum in (signal.SIGINT, signal.SIGTERM)}
    try:
        yield cancellation
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)


def require(condition, message):
    if not condition:
        raise OperationError(message)


def environment():
    # Compose gives ambient variables precedence over --env-file. An unrelated
    # shell must never redirect an operation to another project or credentials.
    return {key: value for key, value in os.environ.items()
            if key == "BEWEFT_TOKEN" or
            (not key.startswith(("BEWEFT_", "COMPOSE_")) and key != "DATABASE_URL")}


def call(args, *, capture=True, stdin=None, stdout=None, ok=True, timeout=None):
    try:
        result = subprocess.run(args, env=environment(), stdin=stdin,
                                stdout=subprocess.PIPE if capture and stdout is None else stdout,
                                stderr=subprocess.PIPE if capture else None, check=False, timeout=timeout,
                                start_new_session=capture)
    except FileNotFoundError as error:
        raise OperationError("Docker with Compose is required.") from error
    except subprocess.TimeoutExpired as error:
        raise OperationError("Docker did not respond before its operation deadline.") from error
    if ok and result.returncode:
        # Diagnostics may contain expanded environment values; do not echo them.
        label = next((value for value in ("pg_restore", "pg_dump", "tar", "up", "start", "stop", "run", "inspect")
                      if value in args), "operation")
        raise OperationError(f"Docker {label} failed (exit {result.returncode}); inspect the selected installation and Docker status.")
    return result


def docker(*args, **kwargs):
    return call(["docker", *args], **kwargs)


def output(*args):
    return docker(*args).stdout.decode("utf-8").strip()


def check_prerequisites():
    docker("compose", "version")


def validate_project(project):
    require(PROJECT_PATTERN.fullmatch(project) is not None,
            "Project must start with a lowercase letter or digit and contain only lowercase letters, digits, _ or - (maximum 51 characters).")


def private_destination(path):
    resolved = path.resolve()
    private = ROOT / ".local"
    require(ROOT not in (resolved, *resolved.parents) or private in (resolved, *resolved.parents),
            "Place installations and backups outside the checkout or under its ignored .local directory.")


def empty_project(project):
    """Check only the explicitly selected project's labels and volume names."""
    validate_project(project)
    for resource in ("container", "volume", "network"):
        args = {"container": ["ps", "-aq"], "volume": ["volume", "ls", "-q"],
                "network": ["network", "ls", "-q"]}[resource]
        require(not output(*args, "--filter", f"label=com.docker.compose.project={project}"),
                f"The destination Compose project already has {resource} resources; choose an empty project.")
    for suffix in ("database", "media"):
        found = docker("volume", "inspect", f"{project}_{suffix}", ok=False)
        require(found.returncode != 0, "A destination volume name already exists; choose an empty project.")
    found = docker("network", "inspect", f"{project}_default", ok=False)
    require(found.returncode != 0, "A destination network name already exists; choose an empty project.")


def setup(destination, project, port):
    check_prerequisites()
    validate_project(project)
    require(1024 <= port <= 65535, "Port must be between 1024 and 65535.")
    path = Path(destination).expanduser().absolute()
    private_destination(path)
    require(not os.path.lexists(path), "Installation already exists; its files and secrets were preserved.")
    empty_project(project)
    path.mkdir(mode=0o700)
    values = {
        "COMPOSE_PROJECT_NAME": project,
        "BEWEFT_IMAGE": "unconfigured",
        "BEWEFT_WORKER_ACTIVE": "true",
        "BEWEFT_PORT": str(port),
        "BEWEFT_BIND_IP": "127.0.0.1",
        "BEWEFT_PUBLIC_URL": f"http://127.0.0.1:{port}",
        "BEWEFT_MAX_UPLOAD_BYTES": "268435456",
        "BEWEFT_MEDIA_QUOTA_BYTES": "10737418240",
        "BEWEFT_DB_PASSWORD": secrets.token_hex(32),
        "BEWEFT_SETUP_SECRET": secrets.token_hex(32),
        "BEWEFT_ENCRYPTION_KEY": secrets.token_hex(32),
    }
    fd = os.open(path / "installation.env", os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as file:
        file.writelines(f"{key}={value}\n" for key, value in values.items())
    print(f"Installation configuration created: {path / 'installation.env'}")
    print("Store installation.env privately and separately from backups; it contains the encryption key.")
    print(f"Next: ./project build '{path}' (or ./project image '{path}' <local-image>)")
    print(f"Then: ./project compose '{path}' up -d --wait --no-build --pull never")
    print("Use the setup secret from the private file to create the first owner.")


class Installation:
    def __init__(self, directory):
        self.path = Path(directory).expanduser().resolve(strict=True)
        require(self.path.is_dir(), "Specify an existing installation directory.")
        self.envfile = self.path / "installation.env"
        info = self.envfile.lstat()
        require(stat.S_ISREG(info.st_mode) and info.st_mode & 0o077 == 0,
                "installation.env must be a private regular file (chmod 600).")
        self.values = {}
        for line in self.envfile.read_text(encoding="utf-8").splitlines():
            if not line or line.startswith("#"):
                continue
            key, sep, value = line.partition("=")
            require(sep and re.fullmatch(r"[A-Z][A-Z0-9_]*", key) and key not in self.values,
                    "installation.env contains a malformed or duplicate setting.")
            require(not any(ch in value for ch in "\r\n$'\"\\"), "installation.env values must be unquoted literals without interpolation.")
            self.values[key] = value
        required = ("COMPOSE_PROJECT_NAME", "BEWEFT_IMAGE", "BEWEFT_WORKER_ACTIVE", "BEWEFT_PORT", "BEWEFT_BIND_IP", "BEWEFT_PUBLIC_URL",
                    "BEWEFT_DB_PASSWORD", "BEWEFT_SETUP_SECRET", "BEWEFT_ENCRYPTION_KEY")
        require(all(self.values.get(key) for key in required), "installation.env is missing a required setting.")
        self.project = self.values["COMPOSE_PROJECT_NAME"]
        validate_project(self.project)
        require(self.values["BEWEFT_IMAGE"] == "unconfigured" or IMAGE_ID.fullmatch(self.values["BEWEFT_IMAGE"]) is not None,
                "Installation image must be an immutable ID selected with ./project image or build.")
        require(self.values["BEWEFT_WORKER_ACTIVE"] in ("true", "false"), "Worker activation state is missing or invalid; the worker is held.")
        require(HEX64.fullmatch(self.values["BEWEFT_DB_PASSWORD"]) is not None and
                HEX64.fullmatch(self.values["BEWEFT_SETUP_SECRET"]) is not None and
                HEX64.fullmatch(self.values["BEWEFT_ENCRYPTION_KEY"]) is not None,
                "The three generated installation secrets must each be 64 hexadecimal characters.")
        check_prerequisites()

    def compose(self, *args, capture=True, stdin=None, stdout=None, ok=True, timeout=None):
        profile = ["--profile", "worker"] if self.values["BEWEFT_WORKER_ACTIVE"] == "true" else []
        return docker("compose", "--env-file", str(self.envfile), "--project-name", self.project,
                      "-f", str(COMPOSE), *profile, *args, capture=capture, stdin=stdin, stdout=stdout, ok=ok, timeout=timeout)

    @contextmanager
    def locked(self):
        fd = os.open(self.path / ".beweft.lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        try:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError as error:
                raise OperationError("Another operation is using this installation.") from error
            yield
        finally:
            os.close(fd)

    def key_digest(self):
        # Matches format 1 backups made by the original operator helper.
        return hashlib.sha256((self.values["BEWEFT_ENCRYPTION_KEY"] + "\n").encode()).hexdigest()

    def image_id(self, image):
        require(IMAGE_PATTERN.fullmatch(image) is not None, "Invalid image reference.")
        result = output("image", "inspect", "--format", "{{.Id}}", image)
        require(IMAGE_ID.fullmatch(result) is not None, "Docker returned an invalid image ID.")
        return result

    def replace_settings(self, updates):
        require(set(updates) <= {"BEWEFT_IMAGE", "BEWEFT_WORKER_ACTIVE"}, "Invalid installation update.")
        if "BEWEFT_IMAGE" in updates:
            require(IMAGE_ID.fullmatch(updates["BEWEFT_IMAGE"]) is not None, "Image must be an immutable local image ID.")
        if "BEWEFT_WORKER_ACTIVE" in updates:
            require(updates["BEWEFT_WORKER_ACTIVE"] in ("true", "false"), "Invalid worker activation state.")
        lines = self.envfile.read_text(encoding="utf-8").splitlines()
        lines = [line.split("=", 1)[0] + "=" + updates[line.split("=", 1)[0]]
                 if line.split("=", 1)[0] in updates else line for line in lines]
        fd, name = tempfile.mkstemp(prefix=".installation-", dir=self.path)
        try:
            with os.fdopen(fd, "w", encoding="utf-8") as file:
                file.write("\n".join(lines) + "\n")
                file.flush()
                os.fsync(file.fileno())
            os.replace(name, self.envfile)
        finally:
            if os.path.exists(name):
                os.unlink(name)
        self.values.update(updates)


def sha256(file, deadline=None, cancellation=None):
    digest = hashlib.sha256()
    file.seek(0)
    while chunk := file.read(1024 * 1024):
        if cancellation is not None:
            cancellation.check()
        if deadline is not None:
            require(time.monotonic() < deadline, "The backup downtime limit expired; restart of the original services was requested.")
        digest.update(chunk)
    file.seek(0)
    return digest.hexdigest()


def wait_owned(process, deadline, cancellation):
    while process.poll() is None:
        cancellation.check()
        require(time.monotonic() < deadline, "The backup downtime limit expired; recovering the original services.")
        time.sleep(0.05)
    cancellation.check()
    require(time.monotonic() < deadline, "The backup downtime limit expired; recovering the original services.")
    return process.returncode


def cleanup_remaining(cleanup):
    if cleanup[0] is None:
        cleanup[0] = time.monotonic() + CLEANUP_SECONDS
    remaining = cleanup[0] - time.monotonic()
    if remaining <= 0:
        raise StopUncertain("Cleanup deadline expired; automatic restart was withheld; inspect the actual Docker state before recovery.")
    return remaining


def wait_cleanup(process, cleanup):
    try:
        return process.wait(timeout=cleanup_remaining(cleanup))
    except subprocess.TimeoutExpired as error:
        process.kill()
        process.wait()
        raise StopUncertain("A Docker request did not complete; automatic restart was withheld; inspect the actual Docker state before recovery.") from error


def container_state(container, cleanup=None):
    try:
        result = docker("inspect", "--format", "{{json .State}}", container,
                        timeout=cleanup_remaining(cleanup) if cleanup is not None else 10).stdout.decode()
        import json
        return json.loads(result)
    except (OperationError, ValueError, UnicodeError) as error:
        raise StopUncertain("Could not verify container state; automatic restart was withheld; inspect the actual Docker state before recovery.") from error


def finish_stop(process, container, cleanup, force):
    try:
        if force:
            docker("kill", "--signal", "KILL", container, ok=False, timeout=cleanup_remaining(cleanup))
            result = wait_cleanup(process, cleanup)
        else:
            result = process.wait()
        if result != 0:
            raise StopUncertain("The original Docker stop request did not complete successfully; automatic restart was withheld.")
        state = container_state(container, cleanup if force else None)
        if state.get("Running") or state.get("Restarting") or state.get("Status") not in ("exited", "dead"):
            raise StopUncertain("The original stop request did not reach a terminal state; automatic restart was withheld; inspect the actual Docker state before recovery.")
    except StopUncertain:
        if process.poll() is None:
            process.kill()
            process.wait()
        raise
    except OperationError as error:
        if process.poll() is None:
            process.kill()
            process.wait()
        raise StopUncertain("Could not complete the original stop request; automatic restart was withheld; inspect the actual Docker state before recovery.") from error


def stop_service(install, service, deadline, cancellation, cleanup, stops):
    cancellation.check()
    container = install.compose("ps", "-q", service).stdout.decode().strip()
    require(container, f"The running {service} container was not found.")
    state = container_state(container)
    require(state.get("Running") and not state.get("Restarting"), f"The {service} container is not running normally.")
    cancellation.check()
    record = {"container": container, "started": state.get("StartedAt"), "confirmed": False}
    stops[service] = record
    process = subprocess.Popen(["docker", "stop", "--time", "360", container], env=environment(),
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    try:
        result = wait_owned(process, deadline, cancellation)
    except (Cancelled, OperationError):
        finish_stop(process, container, cleanup, force=True)
        record["confirmed"] = True
        raise
    if result != 0:
        finish_stop(process, container, cleanup, force=True)
        record["confirmed"] = True
        raise OperationError(f"Could not stop {service}; original services will be recovered.")
    finish_stop(process, container, cleanup, force=False)
    record["confirmed"] = True
    cancellation.check()
    require(time.monotonic() < deadline, "The backup downtime limit expired; recovering the original services.")


def cancel_dump_backend(install, app_name, cleanup):
    # app_name is generated locally as a unique lowercase hex identifier.
    predicate = "application_name='" + app_name + "' AND datname=current_database() AND pid<>pg_backend_pid()"
    terminate = "SELECT pg_terminate_backend(pid) FROM pg_catalog.pg_stat_activity WHERE " + predicate
    count = "SELECT count(*) FROM pg_catalog.pg_stat_activity WHERE " + predicate
    try:
        install.compose("exec", "-T", "db", "psql", "-qAt", "-v", "ON_ERROR_STOP=1", "-U", "beweft", "-d", "beweft",
                        "-c", terminate, timeout=cleanup_remaining(cleanup))
        while True:
            result = install.compose("exec", "-T", "db", "psql", "-qAt", "-v", "ON_ERROR_STOP=1", "-U", "beweft", "-d", "beweft",
                                     "-c", count, timeout=cleanup_remaining(cleanup)).stdout.decode().strip()
            if result == "0":
                return
            time.sleep(0.1)
    except (OperationError, UnicodeError) as error:
        raise StopUncertain(f"Could not confirm cancellation of dump session {app_name}; automatic restart was withheld; inspect the database and Docker state before recovery.") from error


def stream_helper(install, name, create_args, output_file, deadline, cancellation, cleanup, app_name=None, extra_env=None):
    cancellation.check()
    env = dict(environment(), **(extra_env or {}))
    create = subprocess.Popen(["docker", "create", "--name", name, *create_args], env=env,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    created = False
    started = None
    try:
        try:
            result = wait_owned(create, deadline, cancellation)
        except (Cancelled, OperationError):
            # The create request can still be in the daemon: wait for its result
            # before removing the uniquely named helper and restarting services.
            try:
                result = wait_cleanup(create, cleanup)
            except StopUncertain as error:
                raise StopUncertain(f"Backup helper {name} creation may still be pending; automatic restart was withheld; inspect the named helper and Docker state before recovery.") from error
            created = result == 0
            if not created:
                raise StopUncertain(f"Could not confirm the outcome of backup helper {name} creation; automatic restart was withheld.")
            raise
        if result != 0:
            raise StopUncertain(f"Could not confirm the outcome of backup helper {name} creation; automatic restart was withheld.")
        created = True
        cancellation.check()
        started = subprocess.Popen(["docker", "start", "--attach", name], env=environment(),
                                   stdout=output_file, stderr=subprocess.DEVNULL, start_new_session=True)
        try:
            result = wait_owned(started, deadline, cancellation)
        except (Cancelled, OperationError):
            try:
                docker("rm", "-f", name, timeout=cleanup_remaining(cleanup))
                created = False
                wait_cleanup(started, cleanup)
                if app_name:
                    cancel_dump_backend(install, app_name, cleanup)
                    app_name = None
            except StopUncertain:
                raise
            except OperationError as error:
                raise StopUncertain(f"Could not cancel backup helper {name}; automatic restart was withheld; inspect the named helper and Docker state before recovery.") from error
            raise
        require(result == 0, "The backup helper failed; original services will be recovered.")
        cancellation.check()
    finally:
        failed = sys.exc_info()[0] is not None
        if created:
            try:
                timeout = cleanup_remaining(cleanup) if failed else max(0.1, deadline - time.monotonic())
                docker("rm", "-f", name, timeout=timeout)
            except OperationError as error:
                if started is not None and started.poll() is None:
                    started.kill()
                    started.wait()
                raise StopUncertain(f"Could not remove backup helper {name}; automatic restart was withheld; inspect the named helper and Docker state before recovery.") from error
        if failed and app_name and started is not None:
            cancel_dump_backend(install, app_name, cleanup)
    cancellation.check()


def restart_original(install, stops, cleanup):
    try:
        install.compose("start", "--wait", "--wait-timeout", "120", "api", "worker", timeout=cleanup_remaining(cleanup))
        api = stops.get("api")
        if api and api["confirmed"]:
            state = container_state(api["container"], cleanup)
            require(state.get("Running") and state.get("StartedAt") != api["started"],
                    "API did not start a new process after the completed stop.")
            docker("exec", api["container"], "wget", "-q", "-T", "5", "-O", "/dev/null",
                   "http://127.0.0.1:3000/healthz", timeout=cleanup_remaining(cleanup))
    except (OperationError, OSError) as error:
        raise OperationError("Could not verify restarted API and worker; check the installation before continuing.") from error


def snapshot(install, destination, *, resume, max_downtime):
    with termination_guard() as cancellation:
        return snapshot_guarded(install, destination, resume=resume, max_downtime=max_downtime, cancellation=cancellation)


def snapshot_guarded(install, destination, *, resume, max_downtime, cancellation):
    require(1 <= max_downtime <= 86400, "Backup downtime limit must be between 1 and 86400 seconds.")
    destination = Path(destination).expanduser().absolute()
    private_destination(destination)
    require(not os.path.lexists(destination), "Backup destination already exists; refusing overwrite.")
    running = set(install.compose("ps", "--status", "running", "--services").stdout.decode().splitlines())
    require({"db", "api", "worker"} <= running, "Database, API and worker must be running before backup.")
    install.image_id("postgres:17-alpine")  # The dump helper must be available before stopping services.
    api = install.compose("ps", "-q", "api").stdout.decode().strip()
    require(api, "Running API container was not found.")
    media = output("inspect", api, "--format", '{{range .Mounts}}{{if eq .Destination "/app/media"}}{{.Name}}{{end}}{{end}}')
    require(media == f"{install.project}_media", "The selected API does not use its expected named media volume.")
    image = output("inspect", api, "--format", "{{.Image}}")
    require(IMAGE_ID.fullmatch(image) is not None, "The running API image ID is invalid.")
    destination.mkdir(mode=0o700)
    succeeded = False
    safe_restart = True
    stops = {}
    cleanup = [None]
    deadline = time.monotonic() + max_downtime
    try:
        print("Stopping API, then worker, to capture a consistent database and media boundary.", flush=True)
        stop_service(install, "api", deadline, cancellation, cleanup, stops)
        stop_service(install, "worker", deadline, cancellation, cleanup, stops)
        nonce = secrets.token_hex(12)
        helper = install.project + "-dump-" + nonce
        app_name = "beweft-backup-" + nonce
        with (destination / "database.dump").open("xb") as file:
            stream_helper(install, helper, ["--pull=never", "--network", f"{install.project}_default", "--env", "PGPASSWORD", "--env", "PGAPPNAME",
                                           "postgres:17-alpine", "pg_dump", "-h", "db", "-U", "beweft", "-d", "beweft", "-Fc"],
                          file, deadline, cancellation, cleanup, app_name=app_name,
                          extra_env={"PGPASSWORD": install.values["BEWEFT_DB_PASSWORD"], "PGAPPNAME": app_name})
        helper = install.project + "-media-" + secrets.token_hex(12)
        with (destination / "media.tar").open("xb") as file:
            stream_helper(install, helper, ["--pull=never", "--network", "none", "--read-only", "--mount",
                                           f"type=volume,src={media},dst=/media,readonly", "--entrypoint", "tar", image,
                                           "-C", "/media", "-cf", "-", "."],
                          file, deadline, cancellation, cleanup)
        (destination / "key.sha256").write_text(install.key_digest() + "\n", encoding="ascii")
        (destination / "image-id").write_text(image + "\n", encoding="ascii")
        (destination / "manifest").write_text("format=1\ncreated_utc=" + datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ") + "\n", encoding="ascii")
        with (destination / "SHA256SUMS").open("x", encoding="ascii") as sums:
            for name in BACKUP_FILES:
                cancellation.check()
                with (destination / name).open("rb") as file:
                    sums.write(f"{sha256(file, deadline, cancellation)}  {name}\n")
        cancellation.check()
        require(time.monotonic() < deadline, "The backup downtime limit expired; recovering the original services.")
        succeeded = True
    except StopUncertain:
        safe_restart = False
        raise
    finally:
        if (resume or not succeeded or cancellation.signal) and safe_restart:
            restart_original(install, stops, cleanup)
        if not safe_restart:
            print("Automatic restart was withheld because cleanup could not be proven complete.", file=sys.stderr)
        if not succeeded:
            print(f"Backup did not complete; inspect and remove the incomplete directory when safe: {destination}", file=sys.stderr)
    cancellation.check()
    print(f"Backup completed: {destination}")
    print("The backup contains private content. Keep its matching installation.env separately.")
    return image


def safe_media_members(file):
    """Read only fixed tar headers; reject extension records before allocating them."""
    members = []
    names = {}
    total = 0
    entries = 0
    file.seek(0, os.SEEK_END)
    length = file.tell()
    file.seek(0)
    require(length % tarfile.BLOCKSIZE == 0, "Media archive has a truncated block.")
    while True:
        header = file.read(tarfile.BLOCKSIZE)
        require(len(header) == tarfile.BLOCKSIZE, "Media archive has no complete end marker.")
        if header == bytes(tarfile.BLOCKSIZE):
            require(file.read(tarfile.BLOCKSIZE) == bytes(tarfile.BLOCKSIZE), "Media archive has an incomplete end marker.")
            while block := file.read(1024 * 1024):
                require(not any(block), "Media archive contains data after its end marker.")
            break
        entries += 1
        require(entries <= 100000, "Media archive has too many entries.")
        member = tarfile.TarInfo.frombuf(header, encoding="utf-8", errors="strict")
        name = member.name
        require(name and not name.startswith("/") and "\\" not in name and "\x00" not in name,
                "Media archive contains an unsafe path.")
        parts = name.split("/")
        require(".." not in parts, "Media archive contains a parent path.")
        normalized = "/".join(part for part in parts if part not in ("", "."))
        require(member.type in (tarfile.REGTYPE, tarfile.AREGTYPE, tarfile.DIRTYPE) and not member.sparse,
                "Media archive may contain only ordinary files and directories.")
        require(member.size >= 0 and (not member.isdir() or member.size == 0), "Invalid media archive entry size.")
        offset = file.tell()
        next_header = offset + ((member.size + tarfile.BLOCKSIZE - 1) // tarfile.BLOCKSIZE) * tarfile.BLOCKSIZE
        require(next_header <= length, "Media archive entry is truncated.")
        file.seek(next_header)
        if not normalized:
            require(member.isdir(), "Media archive root must be a directory.")
            continue
        require(normalized not in names, "Media archive contains duplicate paths.")
        names[normalized] = member.isdir()
        total += member.size
        require(total <= 1024 ** 4, "Media archive exceeds the 1 TiB restore bound.")
        members.append((normalized, member, offset))
    for name in names:
        parent = PurePosixPath(name).parent
        while str(parent) != ".":
            require(names.get(str(parent), True), "Media archive has a file where a directory is required.")
            parent = parent.parent
    return members


@contextmanager
def verified_backup(directory, install):
    directory = Path(directory).expanduser().resolve(strict=True)
    require(directory.is_dir(), "Backup must be a directory.")
    require({item.name for item in directory.iterdir()} == set(BACKUP_FILES) | {"SHA256SUMS"},
            "Backup must contain exactly the expected format 1 files.")
    with ExitStack() as stack:
        files = {}
        for name in (*BACKUP_FILES, "SHA256SUMS"):
            fd = os.open(directory / name, os.O_RDONLY | os.O_NOFOLLOW)
            require(stat.S_ISREG(os.fstat(fd).st_mode), "Backup entries must be regular files.")
            files[name] = stack.enter_context(os.fdopen(fd, "rb"))
        require(os.fstat(files["SHA256SUMS"].fileno()).st_size < 4096, "Invalid checksum file.")
        for name in ("manifest", "key.sha256", "image-id"):
            require(os.fstat(files[name].fileno()).st_size <= 1024, "Invalid backup metadata size.")
        checks = files["SHA256SUMS"].read().decode("ascii").splitlines()
        expected = {}
        for line in checks:
            match = re.fullmatch(r"([a-f0-9]{64})  ([a-zA-Z0-9_.-]+)", line)
            require(match is not None and match.group(2) not in expected, "Invalid checksum list.")
            expected[match.group(2)] = match.group(1)
        require(set(expected) == set(BACKUP_FILES), "Checksum list does not match the backup format.")
        for name in BACKUP_FILES:
            require(sha256(files[name]) == expected[name], "Backup checksum verification failed.")
        manifest = files["manifest"].read(1024).decode("ascii").splitlines()
        require(manifest and manifest[0] == "format=1", "Unsupported backup format.")
        fingerprint = files["key.sha256"].read(128).decode("ascii").strip()
        require(fingerprint == install.key_digest(), "Encryption key does not match the backup; no resources were changed.")
        image = files["image-id"].read(128).decode("ascii").strip()
        require(IMAGE_ID.fullmatch(image) is not None, "Backup image ID is invalid.")
        require(install.image_id(install.values["BEWEFT_IMAGE"]) == image,
                "Configure the original backup image before restoring; no resources were changed.")
        members = safe_media_members(files["media.tar"])
        yield files, members


def extract_media(install, media, members):
    volume = f"{install.project}_media"
    exists = docker("volume", "inspect", volume, ok=False)
    if exists.returncode:
        docker("volume", "create", "--label", f"com.docker.compose.project={install.project}",
               "--label", "com.docker.compose.volume=media", volume)
    # Alpine's own /media contains placeholder directories; do not copy them
    # into a new application volume on its first mount.
    mount = f"type=volume,src={volume},dst=/media,volume-nocopy"
    image = install.values["BEWEFT_IMAGE"]
    docker("run", "--rm", "--pull=never", "--network", "none", "--read-only", "--user", "0:0", "--mount", mount,
           "--entrypoint", "sh", image, "-c", 'test -z "$(ls -A /media)" && chown 10001:10001 /media')
    helper = f"{install.project}-restore-{secrets.token_hex(6)}"
    process = subprocess.Popen(["docker", "run", "--rm", "-i", "--name", helper, "--pull=never", "--network", "none", "--read-only",
                                "--user", "0:0", "--mount", mount, "--entrypoint", "tar", image, "-C", "/media", "-xf", "-"],
                               env=environment(), stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        # Emit a new archive with only validated names, regular content and fixed
        # ownership/modes. Never feed the original archive to the extractor.
        with tarfile.open(fileobj=process.stdin, mode="w|", format=tarfile.PAX_FORMAT) as clean:
            for name, member, offset in members:
                entry = tarfile.TarInfo(name)
                entry.type = tarfile.DIRTYPE if member.isdir() else tarfile.REGTYPE
                entry.mode = 0o755 if member.isdir() else 0o644
                entry.uid = entry.gid = 10001
                entry.size = 0 if member.isdir() else member.size
                media.seek(offset)
                clean.addfile(entry, None if member.isdir() else media)
        process.stdin.close()
        require(process.wait() == 0, "Media extraction failed; destination remains stopped.")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        docker("rm", "-f", helper, ok=False, timeout=10)


def restore(install, backup):
    allowed = {"installation.env", ".beweft.lock"}
    require({item.name for item in install.path.iterdir()} <= allowed,
            "Restore requires an empty installation directory containing only installation.env.")
    empty_project(install.project)
    with verified_backup(backup, install) as (files, members):
        # Validation may take time for large media; check again immediately
        # before creating the destination project.
        empty_project(install.project)
        install.replace_settings({"BEWEFT_WORKER_ACTIVE": "false"})
        print("Backup, image, key, paths and empty destination verified; restoring into the new project.", flush=True)
        install.compose("up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never", "db")
        install.compose("exec", "-T", "db", "pg_restore", "--exit-on-error", "--single-transaction", "--no-owner",
                        "--no-privileges", "-U", "beweft", "-d", "beweft", stdin=files["database.dump"])
        extract_media(install, files["media.tar"], members)
        install.compose("up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never", "db", "api")
    print("Restore started the database and API; the worker remains stopped.")
    print("Stop the old worker and reconcile all queued, overdue, sending and uncertain publications before activation.")


def upgrade(install, image, backup, max_downtime):
    new_image = install.image_id(image)
    current = install.image_id(install.values["BEWEFT_IMAGE"])
    require(new_image != current, "The requested image is already configured; choose a different image.")
    api = install.compose("ps", "-q", "api").stdout.decode().strip()
    require(api and output("inspect", api, "--format", "{{.Image}}") == current,
            "Running and configured image differ; reconcile them before upgrade.")
    old_image = snapshot(install, backup, resume=False, max_downtime=max_downtime)
    require(old_image == current, "Running and configured image differ; services remain stopped. Resolve this before upgrade.")
    install.replace_settings({"BEWEFT_IMAGE": new_image})
    print("Starting the new image. If this fails, restore the backup into a new empty project using the old image and key.", flush=True)
    install.compose("up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never", "--force-recreate")
    print(f"Upgrade completed. Prior image ID: {old_image}. Preserve that image and the matching backup for rollback.")


def select_image(install, image):
    empty_project(install.project)
    require(install.values["BEWEFT_IMAGE"] == "unconfigured", "Image selection is only for a new installation; use upgrade afterward.")
    selected = install.image_id(image)
    install.replace_settings({"BEWEFT_IMAGE": selected})
    print(f"Pinned local application image: {selected}")


def build_image(install, node_image):
    empty_project(install.project)
    require(install.values["BEWEFT_IMAGE"] == "unconfigured", "Image build is only for a new installation; use upgrade afterward.")
    tag = f"beweft:{install.project}-{secrets.token_hex(6)}"
    command = ["build", "--pull=false", "-t", tag]
    if node_image:
        require(IMAGE_PATTERN.fullmatch(node_image) is not None, "Invalid Node base image reference.")
        command += ["--build-arg", "NODE_IMAGE=" + node_image]
    docker(*command, str(ROOT), capture=False)
    selected = install.image_id(tag)
    install.replace_settings({"BEWEFT_IMAGE": selected})
    print(f"Built {tag} and pinned application image: {selected}")


def activate(install, reviewed):
    require(reviewed, "Activation requires --handover-reviewed after stopping the old worker and reconciling every restored queued, overdue, sending and uncertain publication.")
    require(install.values["BEWEFT_WORKER_ACTIVE"] == "false", "The worker is already enabled.")
    require(IMAGE_ID.fullmatch(install.values["BEWEFT_IMAGE"]) is not None, "Select an immutable image before activation.")
    # This is an explicit operator attestation, not proof of remote outcomes.
    install.replace_settings({"BEWEFT_WORKER_ACTIVE": "true"})
    print("Worker activation requested; it may publish queued items. If startup fails, inspect its state before retrying.", flush=True)
    install.compose("up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never", "--force-recreate", "worker")


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    p = commands.add_parser("setup")
    p.add_argument("directory")
    p.add_argument("project", nargs="?", default="beweft")
    p.add_argument("port", nargs="?", type=int, default=3000)
    p = commands.add_parser("compose")
    p.add_argument("directory")
    p.add_argument("args", nargs=argparse.REMAINDER)
    p = commands.add_parser("image")
    p.add_argument("directory")
    p.add_argument("image")
    p = commands.add_parser("build")
    p.add_argument("directory")
    p.add_argument("--node-image", default=None)
    p = commands.add_parser("activate")
    p.add_argument("directory")
    p.add_argument("--handover-reviewed", action="store_true")
    p = commands.add_parser("backup")
    p.add_argument("directory")
    p.add_argument("destination")
    p.add_argument("--max-downtime", type=int, default=1800, metavar="SECONDS")
    p = commands.add_parser("restore")
    p.add_argument("directory")
    p.add_argument("backup")
    p = commands.add_parser("upgrade")
    p.add_argument("directory")
    p.add_argument("image")
    p.add_argument("backup")
    p.add_argument("--max-downtime", type=int, default=1800, metavar="SECONDS")
    args = parser.parse_args()
    if args.command == "setup":
        setup(args.directory, args.project, args.port)
        return
    install = Installation(args.directory)
    with install.locked():
        if args.command == "compose":
            require(args.args, "Supply Compose arguments.")
            require("config" not in args.args or args.args == ["config", "--quiet"],
                    "Expanded Compose config exposes secrets. Use config --quiet.")
            require("build" not in args.args and "--build" not in args.args,
                    "Build separately with ./project build; normal Compose commands never rebuild an installation image.")
            if any(item in {"up", "run", "start", "restart", "create"} for item in args.args):
                require(IMAGE_ID.fullmatch(install.values["BEWEFT_IMAGE"]) is not None,
                        "Select an immutable image with ./project image or build before starting services.")
            result = install.compose(*args.args, capture=False, ok=False)
            if result.returncode:
                raise OperationError(f"Compose exited with status {result.returncode}.")
        elif args.command == "backup":
            snapshot(install, args.destination, resume=True, max_downtime=args.max_downtime)
        elif args.command == "restore":
            restore(install, args.backup)
        elif args.command == "upgrade":
            upgrade(install, args.image, args.backup, args.max_downtime)
        elif args.command == "image":
            select_image(install, args.image)
        elif args.command == "build":
            build_image(install, args.node_image)
        elif args.command == "activate":
            activate(install, args.handover_reviewed)


if __name__ == "__main__":
    try:
        main()
    except (OperationError, OSError, UnicodeError, tarfile.TarError, ValueError) as error:
        print(f"beweft: {error}", file=sys.stderr)
        sys.exit(1)
