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


class OperationError(Exception):
    pass


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
                                stderr=subprocess.PIPE if capture else None, check=False, timeout=timeout)
    except FileNotFoundError as error:
        raise OperationError("Docker with Compose is required.") from error
    except subprocess.TimeoutExpired as error:
        raise OperationError("The backup downtime limit expired; restart of the original services was requested.") from error
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
        "BEWEFT_IMAGE": "beweft:local",
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
    print(f"Next: ./project compose '{path}' up -d --build --wait")
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
        required = ("COMPOSE_PROJECT_NAME", "BEWEFT_IMAGE", "BEWEFT_PORT", "BEWEFT_BIND_IP", "BEWEFT_PUBLIC_URL",
                    "BEWEFT_DB_PASSWORD", "BEWEFT_SETUP_SECRET", "BEWEFT_ENCRYPTION_KEY")
        require(all(self.values.get(key) for key in required), "installation.env is missing a required setting.")
        self.project = self.values["COMPOSE_PROJECT_NAME"]
        validate_project(self.project)
        require(IMAGE_PATTERN.fullmatch(self.values["BEWEFT_IMAGE"]) is not None, "Invalid BEWEFT_IMAGE.")
        require(HEX64.fullmatch(self.values["BEWEFT_DB_PASSWORD"]) is not None and
                HEX64.fullmatch(self.values["BEWEFT_SETUP_SECRET"]) is not None and
                HEX64.fullmatch(self.values["BEWEFT_ENCRYPTION_KEY"]) is not None,
                "The three generated installation secrets must each be 64 hexadecimal characters.")
        check_prerequisites()

    def compose(self, *args, capture=True, stdin=None, stdout=None, ok=True, timeout=None):
        return docker("compose", "--env-file", str(self.envfile), "--project-name", self.project,
                      "-f", str(COMPOSE), *args, capture=capture, stdin=stdin, stdout=stdout, ok=ok, timeout=timeout)

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

    def replace_image(self, image):
        require(IMAGE_ID.fullmatch(image) is not None, "Upgrade image must be an immutable local image ID.")
        lines = self.envfile.read_text(encoding="utf-8").splitlines()
        lines = ["BEWEFT_IMAGE=" + image if line.startswith("BEWEFT_IMAGE=") else line for line in lines]
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
        self.values["BEWEFT_IMAGE"] = image


def sha256(file, deadline=None):
    digest = hashlib.sha256()
    file.seek(0)
    while chunk := file.read(1024 * 1024):
        if deadline is not None:
            require(time.monotonic() < deadline, "The backup downtime limit expired; restart of the original services was requested.")
        digest.update(chunk)
    file.seek(0)
    return digest.hexdigest()


def snapshot(install, destination, *, resume, max_downtime):
    require(1 <= max_downtime <= 86400, "Backup downtime limit must be between 1 and 86400 seconds.")
    destination = Path(destination).expanduser().absolute()
    private_destination(destination)
    require(not os.path.lexists(destination), "Backup destination already exists; refusing overwrite.")
    running = set(install.compose("ps", "--status", "running", "--services").stdout.decode().splitlines())
    require({"db", "api", "worker"} <= running, "Database, API and worker must be running before backup.")
    api = install.compose("ps", "-q", "api").stdout.decode().strip()
    require(api, "Running API container was not found.")
    media = output("inspect", api, "--format", '{{range .Mounts}}{{if eq .Destination "/app/media"}}{{.Name}}{{end}}{{end}}')
    require(media == f"{install.project}_media", "The selected API does not use its expected named media volume.")
    image = output("inspect", api, "--format", "{{.Image}}")
    require(IMAGE_ID.fullmatch(image) is not None, "The running API image ID is invalid.")
    destination.mkdir(mode=0o700)
    succeeded = False
    deadline = time.monotonic() + max_downtime
    def remaining():
        value = deadline - time.monotonic()
        require(value > 0, "The backup downtime limit expired; restart of the original services was requested.")
        return value
    try:
        print("Stopping API, then worker, to capture a consistent database and media boundary.", flush=True)
        install.compose("stop", "-t", "360", "api", timeout=remaining())
        install.compose("stop", "-t", "360", "worker", timeout=remaining())
        with (destination / "database.dump").open("xb") as file:
            install.compose("exec", "-T", "db", "pg_dump", "-U", "beweft", "-d", "beweft", "-Fc", stdout=file, timeout=remaining())
        helper = f"{install.project}-backup-{secrets.token_hex(6)}"
        try:
            with (destination / "media.tar").open("xb") as file:
                docker("run", "--rm", "--name", helper, "--pull=never", "--network", "none", "--read-only", "--mount",
                       f"type=volume,src={media},dst=/media,readonly", "--entrypoint", "tar", image,
                       "-C", "/media", "-cf", "-", ".", stdout=file, timeout=remaining())
        finally:
            docker("rm", "-f", helper, ok=False, timeout=10)
        (destination / "key.sha256").write_text(install.key_digest() + "\n", encoding="ascii")
        (destination / "image-id").write_text(image + "\n", encoding="ascii")
        (destination / "manifest").write_text("format=1\ncreated_utc=" + datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ") + "\n", encoding="ascii")
        with (destination / "SHA256SUMS").open("x", encoding="ascii") as sums:
            for name in BACKUP_FILES:
                with (destination / name).open("rb") as file:
                    sums.write(f"{sha256(file, deadline)}  {name}\n")
        remaining()
        succeeded = True
        print(f"Backup completed: {destination}")
        print("The backup contains private content. Keep its matching installation.env separately.")
        return image
    finally:
        if resume or not succeeded:
            result = install.compose("start", "--wait", "--wait-timeout", "120", "api", "worker", ok=False)
            if result.returncode:
                raise OperationError("Could not restart API and worker; check this installation before continuing.")
        if not succeeded:
            print(f"Backup did not complete; inspect and remove the incomplete directory when safe: {destination}", file=sys.stderr)


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
        print("Backup, image, key, paths and empty destination verified; restoring into the new project.", flush=True)
        install.compose("up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never", "db")
        install.compose("exec", "-T", "db", "pg_restore", "--exit-on-error", "--single-transaction", "--no-owner",
                        "--no-privileges", "-U", "beweft", "-d", "beweft", stdin=files["database.dump"])
        extract_media(install, files["media.tar"], members)
        install.compose("up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never", "db", "api")
    print("Restore started the database and API; the worker remains stopped.")
    print("Verify the restored data, stop the old worker and reconcile uncertain publications before starting this worker.")


def upgrade(install, image, backup, max_downtime):
    new_image = install.image_id(image)
    current = install.image_id(install.values["BEWEFT_IMAGE"])
    require(new_image != current, "The requested image is already configured; choose a different image.")
    api = install.compose("ps", "-q", "api").stdout.decode().strip()
    require(api and output("inspect", api, "--format", "{{.Image}}") == current,
            "Running and configured image differ; reconcile them before upgrade.")
    old_image = snapshot(install, backup, resume=False, max_downtime=max_downtime)
    require(old_image == current, "Running and configured image differ; services remain stopped. Resolve this before upgrade.")
    install.replace_image(new_image)
    print("Starting the new image. If this fails, restore the backup into a new empty project using the old image and key.", flush=True)
    install.compose("up", "-d", "--wait", "--wait-timeout", "120", "--no-build", "--pull", "never", "--force-recreate")
    print(f"Upgrade completed. Prior image ID: {old_image}. Preserve that image and the matching backup for rollback.")


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
            result = install.compose(*args.args, capture=False, ok=False)
            if result.returncode:
                raise OperationError(f"Compose exited with status {result.returncode}.")
        elif args.command == "backup":
            snapshot(install, args.destination, resume=True, max_downtime=args.max_downtime)
        elif args.command == "restore":
            restore(install, args.backup)
        elif args.command == "upgrade":
            upgrade(install, args.image, args.backup, args.max_downtime)


if __name__ == "__main__":
    try:
        main()
    except (OperationError, OSError, UnicodeError, tarfile.TarError, ValueError) as error:
        print(f"beweft: {error}", file=sys.stderr)
        sys.exit(1)
