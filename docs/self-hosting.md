# Self-hosting

This guide runs one owner’s installation on a single Docker host. No social
account or production hostname is configured by default. The examples keep the
API bound to loopback; replace `<installation>`, `<backup>` and image references
with paths and names you choose. Docker Engine, Docker Compose and Python 3.8+
are required on Linux or macOS. The scripts use Unix file permissions and locks.

## Example workspace

The demo creates a **separate** installation and seeds fictional content through
the normal API. It needs a trusted, already-built local application image and
the locally available `postgres:17-alpine` image. Choose an unused directory,
Compose project and port. For example, after building or obtaining your own
local image:

```sh
./project demo create .local/demo beweft-demo 3001 <trusted-local-image>
```

Open `http://localhost:3001` and sign in with the initial example credentials
`demo@example.com` / `BeweftDemo123!`. These credentials are public and are for
this isolated demo only. The directory has its own private generated secrets,
database and media volumes. Never expose this installation to a network or use
these credentials for a real account. The demo command requires an unused
project beginning with `beweft-demo`, a non-3000 port and a new directory; it
never adopts an existing installation. Standard installation ownership and
image-pinning checks still apply.

The sample includes fictional brands, manually prepared X, Instagram and
YouTube channels, images, ideas, templates, drafts, posting rules, a queue and
an exact calendar appointment. The completed handoff is an owner-attested
example explicitly stating that no public post was made. These are **manual**
channels, not provider connections; analytics is unavailable. No provider app,
token or live publication is created. If you later connect a real channel,
normal publishing behavior applies.

Use `localhost` for the demo and `127.0.0.1` for an installation at
`127.0.0.1:3000`: browsers share cookies across ports on the same hostname.
If your other installation also uses `localhost`, use separate browser profiles
or change that installation's origin deliberately before using both. Keep the
demo directory and its database if you want your practice changes to persist.
The usual `./project compose .local/demo up -d --wait` command starts it again.

The command checkpoints completed API writes in a private marker tied to the
installation and pinned image. `./project demo resume .local/demo` resumes a
partially completed seed only while that owned API is running. Once seeding is
complete, resume does nothing: it does not reset the password or replace edits.
If you changed the owner password before an interrupted seed finished, pipe the
current password as one UTF-8 line from a private source to
`./project demo resume .local/demo --password-stdin`. The command preserves
spaces, reads at most 1024 password bytes, and never saves or prints it. Do not
put the password in command arguments or shell history. The input must be a
non-interactive stream and provide a line or end of file within ten seconds.
Completed seeds do not read the stream.
If a request was interrupted with an uncertain outcome, resume stops without
retrying it; inspect the demo and its marker before deciding what to do. If
setup stopped before creating a marker, the directory is not adopted on retry;
preserve it and use a new directory and project for a new demo. There is no
automatic reset or deletion. The credentials above describe only the initial
password; a password you later choose remains in effect.

For a disposable integration rehearsal of these guards and the seed, run
`BEWEFT_DEMO_TEST_IMAGE=<trusted-local-image> python3 scripts/tests/demo_rehearsal.py`.
It uses uniquely named local Docker resources and removes them afterward.

## Create a local installation

From the repository checkout:

```sh
./project setup <new-installation-directory> <unique-project-name> 3000
./project build <new-installation-directory>
./project compose <new-installation-directory> up -d --wait --no-build --pull never
```

The destination must not already exist and must be outside the checkout (or
under its ignored `.local` directory). Setup reserves the project name on the
selected Docker daemon with a unique installation identity. Every wrapper
operation checks that reservation and the ownership labels on containers,
networks and volumes before touching the project. A second installation cannot
claim the same name even before either starts. The reservation binds the
configuration to its resolved directory: do not move or copy the installation
directory to operate it elsewhere. Setup refuses existing project resources and
creates `installation.env` with unique secrets and mode 600 in a private directory.
Store a separate protected copy of that file, especially
`BEWEFT_ENCRYPTION_KEY`, in a password manager or offline secret store. Do not
commit it or include it in a shareable backup. Changing the encryption key makes
stored provider credentials unreadable.

Setup also pins the local `postgres:17-alpine` image by its immutable ID. The
database and backup client use that exact image even if the tag changes. It must
already exist locally; setup does not pull it. Pass `--postgres-image
<local-image-or-id>` at setup to select a retained image for a restore. The
PostgreSQL ID is also part of the reservation, so editing the installation file
cannot silently change the database image.

`build` creates a uniquely tagged local image, `beweft:<project>-<12 hex digits>`,
and records its immutable SHA-256 image ID in the private `installation-state.json`
before any Compose resources are created. Alternatively,
`./project image <installation> <existing-local-image>` pins a previously built
local image. Normal Compose startup cannot build or select a mutable tag, and
only the backup-first upgrade changes the application pin. The database schema is
the 14 SQL migrations in `migrations/` (`0001` through `0014`), applied
automatically at startup; application upgrades are not automatically reversed.
The worker reports readiness through `beweft-worker --healthcheck`, which performs
a 3-second database probe inside a 4-second outer deadline and uses the stable
`compose-primary` ID. A restored worker remains held and fails that check until
explicitly activated.
Use the `./project` wrapper for installation operations; direct Docker Compose
commands bypass its ownership checks. The wrapper accepts `--profile` before a
Compose command and allows managed `up`, `start`, `stop`, `restart`, `down`,
`ps`, `logs`, and `config --quiet` with a limited set of options. It rejects
`run`, `exec`, command and environment overrides, alternate project or Compose
files, image removal and rebuild options. Use the dedicated scoped CLI command
below. Docker administrators can bypass these safeguards directly; the wrapper
does not protect against a person with direct Docker or host access.

Managed `up` requires `-d` or `--wait`. For a live log stream, use for example
`./project compose <installation> logs -f --tail 100 api`; select exactly one
of `api`, `db` or `worker`. The wrapper follows the selected container's ID,
so recreating that service requires starting a new follower. Log follow and an
admitted CLI or MCP session do not hold the installation lock for their entire
lifetime. An operator may therefore stop or upgrade the API while a client is
connected; clients must handle a lost connection and inspect the result before
retrying any write.

The application image and worker activation are managed state, not editable
settings. Do not add `BEWEFT_IMAGE` or `BEWEFT_WORKER_ACTIVE` to
`installation.env` or edit, replace or copy the state file. Missing or malformed
state prevents operations. Only initial image selection and a backup-first
upgrade change the application pin; only explicit activation releases a held
worker. If a state update reports an error, inspect the installation and backup
before retrying because the atomic update may already have completed. If state
is lost, preserve the running installation and recover a verified backup into a
new held installation rather than guessing or reconstructing the old pin.

The reservation remains after `compose down` and across host restarts. To retire
an installation, preserve anything needed, then run `./project compose
<installation> down --volumes --remove-orphans` and `./project release
<installation>`. Release refuses while project resources or expected
volume/network names remain. It removes only the matching control reservation;
the private installation files remain and can no longer start. If the
configuration or directory was lost, do not recreate or adopt it. Inspect
`beweft-reservation-<project>` with `docker volume inspect`, verify its identity
against your protected installation record, and inspect containers, networks
and volumes with the project's `com.docker.compose.project` label and the
expected `<project>_database`, `<project>_media`, `<project>_default`, and
`<project>-db-1`, `<project>-api-1`, `<project>-worker-1` names.
Only after independently establishing that the reservation belongs to the
retired installation and no project resources remain should a Docker
administrator remove that exact reservation volume manually. If ownership is
uncertain, leave it in place and choose a different project name.

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
./project cli <installation> brands
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
custom dump and a media archive, records both image IDs and the encryption-key
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
session before recovery. Cleanup has a separate 150-second Docker-response
budget, with up to eleven additional seconds to terminate and reap a timed-out
local client group. If it cannot
confirm that the stop request and owned dump have finished, it withholds
automatic restart and reports a failed recovery: inspect the named resources
and finish recovery manually. A timed-out Docker request may already have reached
the daemon; inspect the actual service state before retrying. The dump helper's own anonymous PostgreSQL
volumes are removed and their absence checked before services resume. SIGKILL,
a stalled host, filesystem, or Docker
daemon cannot guarantee cleanup or service availability. Plan enough space and
downtime, and monitor service recovery. A successful backup is not proof of
recovery until you restore and exercise it.

SIGINT or SIGTERM is forwarded to the wrapper's own Docker client group for
managed commands, builds, scoped CLI sessions and media restore, then any
remaining local group processes are forcibly stopped. These commands can have
already reached the daemon; inspect the installation and any named helper
reported in an error before retrying. Long-running interactive sessions and
media transfers have no fixed total time limit. A blocked Docker metadata read
has a finite deadline, and restore attempts bounded removal of its uniquely
named media helper after interruption. SIGKILL or a stalled daemon can prevent
confirmed cleanup.

## Restore or roll back

Restore never replaces an existing project or volume. Create a **new** empty
installation with a unique project name and port, then privately set its
`BEWEFT_ENCRYPTION_KEY` to the value from the original installation file and
pin the original application image ID recorded in `image-id` and PostgreSQL image
ID in `postgres-image-id`. Retain both images before upgrading; if either is no
longer in Docker, obtain the exact image from your own trusted archive first. The new
installation may have its own database password and setup secret.

```sh
./project setup <new-empty-installation> <new-project-name> <new-port> --postgres-image <recorded-postgres-image-id>
./project image <new-empty-installation> <recorded-old-image-id>
# Privately set the matching encryption key in installation.env.
./project restore <new-empty-installation> <backup-directory>
```

Before creating target resources, restore checks the backup file list and
checksums, the encryption-key fingerprint, both local image identities, the target
project and volume names, and every media archive header. Media may contain only
ordinary files and directories with safe relative paths; links, special files,
parent paths, duplicate paths and tar extensions are rejected. The validated
media is re-emitted with fixed ownership into the empty volume. Restore records
a persistent worker hold and starts only the database and API. Ordinary startup,
worker restart and host reboot do not release that hold. Verify owner login,
media bytes, drafts, queue and publication history before switching traffic.
Restore's volume initializer and media extractor use uniquely named helpers
labeled with the installation and operation identity. The wrapper checks those
labels before removing a helper and its anonymous mounts; it keeps the named
media volume. If cleanup or a pending create cannot be confirmed, the error
identifies the helper to inspect before retrying.
Image build cache is separate from helper cleanup and is not pruned by these
commands.
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

Format 2 backups include both image IDs. Earlier format 1 backups do not record
the PostgreSQL image and cannot be restored by this wrapper; do not guess that
identity from a current mutable tag. Preserve the original database, image and
backup while planning a separately verified recovery.

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

The application upgrade command does not upgrade PostgreSQL. The pinned database
image cannot be changed in an existing installation with these commands. For a
database image or major-version change, first take and verify a format 2 backup,
retain both old images and the key, and plan an isolated migration using the
official [PostgreSQL upgrade procedures](https://www.postgresql.org/docs/current/upgrading.html)
for the source and target versions. Check major
version, extensions, collation and application compatibility; do not point a new
major version at the old data volume. Provision a separate installation using
`./project setup <new-directory> <new-project> <port> --postgres-image <target-image-id> --worker-held`,
perform and verify the migration and media recovery there, and keep the original
worker stopped until the full publication handover is reconciled. This project
does not automate or yet qualify that database migration. Restore the old
version and backup into another empty project if rollback is needed.

## Image licenses and source

See [image licenses](image-licenses.md). The application source is MIT, while
the image also contains independently licensed Alpine and FFmpeg components,
including GPL/LGPL code. Per-image inventories and notices are included in the
image. Before redistributing an image, provide complete matching source and
required notices; the local build alone is not a redistribution source offer.
