# Self-hosting

This guide runs one owner’s installation on a single Docker host. No social
account or production hostname is configured by default. The examples keep the
API bound to loopback; replace `<installation>`, `<backup>` and image references
with paths and names you choose. Docker Engine, Docker Compose and Python 3.8+
are required on Linux or macOS. The scripts use Unix file permissions and locks.

## Create a local installation

From the repository checkout:

```sh
./project setup <new-installation-directory> <unique-project-name> 3000
./project build <new-installation-directory>
./project compose <new-installation-directory> up -d --wait --no-build --pull never
```

The destination must not already exist and must be outside the checkout (or
under its ignored `.local` directory). The project name must be unique on the
selected Docker daemon; setup refuses existing project resources. Setup creates
`installation.env` with unique secrets and mode 600 inside a private directory.
Store a separate protected copy of that file, especially
`BEWEFT_ENCRYPTION_KEY`, in a password manager or offline secret store. Do not
commit it or include it in a shareable backup. Changing the encryption key makes
stored provider credentials unreadable.

`build` uses a unique local image tag and records its immutable image ID in the
installation file before any Compose resources are created. Alternatively,
`./project image <installation> <existing-local-image>` pins a previously built
local image. Normal Compose startup cannot build or select a mutable tag. To
change an existing installation's image, use the backup-first upgrade command.

Open `http://127.0.0.1:3000`, create the owner with the setup secret from the
private file, and choose an owner password. Use a unique loopback port if 3000
is occupied; the port supplied to setup becomes the public local origin. The
app starts without demo content or provider credentials. The Compose database
and media volumes survive ordinary container restarts. Check the separate API
and worker health signals with:

```sh
./project compose <installation> ps
./project compose <installation> logs --tail 100 api worker
```

Docker administrators can inspect container environments and volumes; protect
the host and Docker socket. Do not post expanded Compose config or logs that
contain private content. The wrapper permits only `config --quiet`, because
normal `config` prints credentials. Ambient `BEWEFT_*` and `COMPOSE_*` values
are ignored by the wrappers so one shell cannot silently select another project;
`BEWEFT_TOKEN` is the exception for the optional CLI container. It is visible to
Docker administrators in that container's environment. See the
[CLI guide](cli.md) for using a protected token file with a host-side CLI.
The worker uses a stable `compose-primary` ID; assign a distinct ID before
adding another worker that shares the database.

To use the optional CLI container, supply a scoped token through a protected
environment and run, for example:

```sh
# Set BEWEFT_TOKEN privately first; do not put the token in command history.
./project compose <installation> --profile tools run --rm -T cli brands
```

For a future server, put a reverse proxy with HTTPS in front of the loopback
port. Set `BEWEFT_PUBLIC_URL` in the private installation file to the selected
real HTTPS origin before provider authorization and restart the services. No
hostname, certificate, production server, provider consent or live publication
is supplied by this project. Preserve the encryption key during these changes.

## Backup

```sh
./project backup <installation> <new-backup-directory>
```

The database, API and worker must be running, and the backup directory must not
exist. The command stops the API first and then the worker, writes a PostgreSQL
custom dump and a media archive, records the running image ID and encryption-key
fingerprint, and restarts both services. Clients cannot write during this
boundary; do not mutate the database or media directly. Worker shutdown allows
up to six minutes for an in-flight attempt, subject to the chosen downtime
budget. When the budget expires, the command may force-stop the draining
container, waits for the original stop request to finish and verifies a newly
accepting API before reporting recovery. An interrupted external publication
can have an uncertain outcome: inspect its history and the provider before any
retry. The archive contains private content but no installation credentials.

The default backup downtime budget is 1800 seconds; set a different budget with
`--max-downtime SECONDS` (1–86400). If the budget expires, the command requests
service recovery and leaves the incomplete backup directory for inspection and
removal. SIGINT and SIGTERM also cancel the command's own dump client and DB
session before recovery. Cleanup has a separate 150-second budget. If it cannot
confirm that the stop request and owned dump have finished, it withholds
automatic restart and reports a failed recovery: inspect the named resources
and finish recovery manually. SIGKILL, a stalled host, filesystem, or Docker
daemon cannot guarantee cleanup or service availability. Plan enough space and
downtime, and monitor service recovery. A successful backup is not proof of
recovery until you restore and exercise it.

## Restore or roll back

Restore never replaces an existing project or volume. Create a **new** empty
installation with a unique project name and port, then privately set its
`BEWEFT_ENCRYPTION_KEY` to the value from the original installation file and
pin the original local image ID recorded in the backup’s
`image-id` file. Retain the original image before upgrading; if it is no longer
in Docker, obtain that exact image from your own trusted archive first. The new
installation may have its own database password and setup secret.

```sh
./project setup <new-empty-installation> <new-project-name> <new-port>
./project image <new-empty-installation> <recorded-old-image-id>
# Privately set the matching encryption key in installation.env.
./project restore <new-empty-installation> <backup-directory>
```

Before creating target resources, restore checks the backup file list and
checksums, the encryption-key fingerprint, the local image identity, the target
project and volume names, and every media archive header. Media may contain only
ordinary files and directories with safe relative paths; links, special files,
parent paths, duplicate paths and tar extensions are rejected. The validated
media is re-emitted with fixed ownership into the empty volume. Restore records
a persistent worker hold and starts only the database and API. Ordinary startup,
worker restart and host reboot do not release that hold. Verify owner login,
media bytes, drafts, queue and publication history before switching traffic.
Stop the original worker, then reconcile **every restored queued and overdue
publication as well as sending and uncertain publications** against the original
installation and the providers. A queued item in the backup may have published
after the backup was taken. Cancel, resolve or intentionally reschedule each
item as appropriate. Only after that handover review, explicitly activate:

```sh
./project activate <new-empty-installation> --handover-reviewed
```

The flag records the operator's attestation; it does not establish provider
outcomes. Activation may publish eligible queued work. If activation fails,
inspect the worker and publication state before retrying. If a restore fails
after target creation, leave that target isolated;
inspect it and explicitly remove its own project and volumes before trying again.

Checksums detect accidental corruption; they do not authenticate the archive.
Only restore a backup from a trusted source. A PostgreSQL dump can contain SQL
that executes with the database role’s privileges, so media validation does not
make an arbitrary dump safe to run.

## Upgrade

Load or build the new application image locally under a **different tag** from
the current image, then give the command a new backup directory. Do not retag
the currently configured image while its services are running:

```sh
./project upgrade <installation> <new-local-image> <new-backup-directory>
```

The command captures a backup before starting the new image and records the old
image ID in that backup. It pins the new image by ID and waits for the services
to become healthy. Preserve the old image, matching backup and private key. The
backup’s `--max-downtime` budget also applies here; startup of the new image has
a separate 120-second Compose health wait. A failed upgrade does not attempt to
run old binaries against a potentially migrated database. Roll back by restoring
the pre-upgrade backup, old image and matching key into a **new empty project**
as described above, verify it, then switch traffic deliberately. This loses any
writes made after the backup boundary. Database migrations are not automatically
reversed, and an old image may not understand a newer database.

## Image licenses and source

See [image licenses](image-licenses.md). The application source is MIT, while
the image also contains independently licensed Alpine and FFmpeg components,
including GPL/LGPL code. Per-image inventories and notices are included in the
image. Before redistributing an image, provide complete matching source and
required notices; the local build alone is not a redistribution source offer.
