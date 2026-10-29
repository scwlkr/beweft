# CLI and self-host operations status

The CLI/MCP API qualification and worker health slices landed on the handoff
branch after independent review. The self-host packaging slice adds operator
commands and a maintained isolated rehearsal; it still requires its own fresh
review before integration. No live provider publication or production hosting
has been verified.

The CLI and MCP server use scoped API tokens, bounded inputs and outputs,
server-backed pagination and authoritative readback. Draft writes do not
implicitly publish. The [CLI guide](cli.md) describes commands, MCP transport,
scope and unknown-write recovery. In the next request-ID and schedule-discovery
slice, address PR #10's nonblocking finding that raw `api` queries reject
percent-encoded values while retaining same-origin and token protections.

The application image contains the built web UI, API, worker, CLI and ffprobe;
it runs the application as UID 10001. Compose provides persistent PostgreSQL
and media volumes, a loopback API port and distinct API/worker health checks.
`./project setup`, `build`, `image`, `compose`, `cli`, `backup`, `restore`, `activate`,
`upgrade` and `release` are documented
in the [self-hosting guide](self-hosting.md). The commands use an explicit
private installation directory and isolate Compose from ambient `BEWEFT_*` and
`COMPOSE_*` overrides, except the optional CLI's `BEWEFT_TOKEN`. Normal startup
uses immutable application and PostgreSQL image IDs. Controlled installation
state holds the approved application image and worker activation; edits to the
ordinary configuration cannot change either. A Docker reservation binds
each project to its installation identity and directory, and its PostgreSQL pin
cannot change through ordinary startup or application upgrade. Only the
backup-first upgrade changes the application pin. Restore validates a matching key
and both local images, an
empty target project, checksums and a bounded, ordinary-file-only media archive
before creating resources. The restored API starts for verification; its worker
is persistently held until an operator stops the old worker, reconciles all
restored queued/overdue, sending and uncertain work and explicitly attests to
that handover. A queued item in a backup may have published later at its source.

`beweft-worker --healthcheck` needs an explicit PostgreSQL URL and a unique,
stable `BEWEFT_WORKER_ID`. It checks the `public` heartbeat table with a
three-second connect/query and four-second outer binary deadline; no scheduler,
migration, media directory or heartbeat write is started by the check. Ambient
`PG*` configuration and password files are ignored in health mode. Compose
uses `compose-primary`, invokes the same secret-loading entrypoint for startup
and health, and limits the whole wrapper to five seconds. A crashed worker's
heartbeat can remain fresh for up to 30 seconds. Assign a different ID to each
worker sharing a database.

The isolated rehearsal is `scripts/tests/self_host_rehearsal.py`; it uses unique
Compose projects, ports, volumes, owner and manual-channel fixtures. It exercises
installation, competing project reservations, restart, backup, helper volume
cleanup, interrupted Docker client groups and blocked media streams, bounded
metadata reads, a live MCP session during backup, archive/key/image/destination
rejection, restore, byte-identical media and queued data, worker-off recovery,
image upgrade and rollback. The synthetic upgrade image differs by a label, so this rehearsal
validates deployment and rollback mechanics rather than a schema-changing
application release. A PostgreSQL migration and a physical host reboot remain
unqualified. Test resources are removed afterward.

The app source is MIT, but the image includes separately licensed Alpine and
FFmpeg software. The tested FFmpeg reports GPL version 3 or later. Per-image
inventories, build metadata and license texts are included; see
[image licenses](image-licenses.md). **An image must not be redistributed until
the distributor supplies the exact corresponding source and all required
notices.** The final useful local installation will be created separately after
integration; production server/domain selection and live provider approval remain
external decisions.
