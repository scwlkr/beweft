# CLI and operations handoff — incomplete, not release-ready

Packaging is in progress and has not completed its delivery rehearsal or fresh
code review. Existing API, workflow, provider and frontend work belongs to other
slices. Do not treat these packaging files as a verified installation.

## Files owned by this slice

- `crates/cli/`: Rust HTTP client, named JSON CLI commands, tool schemas derived
  from the checked-in OpenAPI artifact, initial newline JSON-RPC stdio MCP loop.
- `Cargo.toml`: added `crates/cli` workspace member only; preserve provider member.
- `Cargo.lock`: CLI dependency graph update; preserve concurrent provider changes.
- `Dockerfile`, `compose.yaml`, `.dockerignore`, `.env.example`: initial packaging.
- `scripts/common.sh`, `setup`, `compose`, `backup`, `container-entrypoint`: initial
  operator helpers. No host Cargo/Node required by these helpers.

This slice adds scoped paged server list queries and per-instance worker health.
It does not change server state, provider modules, frontend, or project CLI wrapper.

## Implemented in source, still requiring independent review

The CLI exposes brands, channels, drafts, one post, queue, history, attempts,
worker health, draft create/update, explicit queue/exact scheduling and media
upload. It uses scoped bearer HTTP requests, disallows redirects and off-origin
escape-hatch paths, supports protected token files, bounds input/output sizes,
and never retries a failed write. Paged draft, media, queue and history reads
filter in the scoped API; full post and publication details are separate reads.
Scheduling returns compact publication summaries to agent clients, while the
default API response retains full snapshots.
MCP includes initialize/initialized, ping,
tools/list and tools/call; its upload root is opened at startup and file paths
cannot follow symlinks. `crates/cli/tests/integration.py` exercises the real
isolated API and MCP subprocess, including scope rejection, lost write response,
readback, upload confinement, redirect refusal and protocol errors. These
changes landed on the integration branch after independent review. Worker health
and the packaging work remain subject to their own review and integration.

The image recipe builds web/API/worker/CLI, includes ffprobe through FFmpeg, and
runs as UID 10001. Compose defines persistent DB/media and loopback API access.
Setup generates private unique deployment values and refuses existing projects.
Backup stops API/worker for a DB/media boundary and records a key fingerprint.
These files have not been built or rehearsed together.

`beweft-worker --healthcheck` only needs `DATABASE_URL` and a stable
`BEWEFT_WORKER_ID`. It reads that instance's heartbeat from the database with a
three-second connect/query deadline and a four-second overall probe deadline; it
does not start a worker, run migrations,
create media directories or write a heartbeat. The PostgreSQL URL must explicitly
provide the host (or socket), user and database; its password may be omitted when
the server permits passwordless authentication. Explicit URL TLS and connection
options are supported; set required TLS settings in the URL. Ambient `PG*`
variables and password files are ignored in health mode. Health checks support
the `public` schema: the connection's
current schema must be `public`, and the heartbeat lookup is schema-qualified.
Regular worker configuration is unchanged; its database session must also use
`public` for this healthcheck to describe that worker. A heartbeat is healthy for 30
seconds. A crashed worker's last heartbeat can remain fresh until that window
expires; graceful shutdown removes its row. IDs must be unique per worker sharing
a database and contain 1–128 ASCII letters, digits, `.`, `_` or `-`. A worker
without an ID generates one for local development, but the healthcheck requires
an explicit ID. The default Compose service uses one fixed ID and is not ready
to scale into multiple workers without assigning a different ID to each.

## Explicit unfinished requirements

- Safe archive validation, restore into an explicit empty destination, matching
  encryption key checks, upgrade command and rollback documentation.
- Isolated Docker fresh install/restart/backup/restore/upgrade rehearsal including
  owner access, byte-identical media, drafts, durable queue and no live publishing.
- Production/TLS placeholder instructions, agent configuration and complete
  self-hosting documentation; setup currently has only concise command guidance.
- Dependency copyright/license notices in image and FFmpeg redistribution/source
  obligations. The entire image must not be represented as MIT.
- Project wrapper commands and fresh independent review of all new code.
- In the next CLI request-ID and schedule-discovery slice, address PR #10's
  nonblocking finding that raw `api` queries reject percent-encoded values while
  retaining same-origin and token protections.

The official MCP transport/lifecycle/tool specifications linked in
`operations-requirements.md` were consulted. No production deployment, Docker
app build, Compose project launch, backup or restore occurred in this slice.

Historical checks before continuation: `cargo check -p beweft-cli`,
`cargo test -p beweft-cli` (one URL confinement regression), CLI Clippy with
`-D warnings`, CLI formatting check and POSIX shell syntax checks all passed.
The temporary Rust/Node image-version probes exited and their `--rm` containers
were removed. No task-created background process or test installation remains.
Build output in the shared `target/` is retained for unfinished work.
