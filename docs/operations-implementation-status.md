# CLI and operations handoff — incomplete, not release-ready

Implementation paused at the owner's request before the delivery rehearsal or
fresh code review. Existing API, workflow, provider and frontend work belongs to
other slices. Do not treat these new packaging files as a verified installation.

## Files owned by this slice

- `crates/cli/`: Rust HTTP client, named JSON CLI commands, tool schemas derived
  from the checked-in OpenAPI artifact, initial newline JSON-RPC stdio MCP loop.
- `Cargo.toml`: added `crates/cli` workspace member only; preserve provider member.
- `Cargo.lock`: CLI dependency graph update; preserve concurrent provider changes.
- `Dockerfile`, `compose.yaml`, `.dockerignore`, `.env.example`: initial packaging.
- `scripts/common.sh`, `setup`, `compose`, `backup`, `container-entrypoint`: initial
  operator helpers. No host Cargo/Node required by these helpers.

No `worker.rs`, `worker_main.rs`, server state, business logic, provider modules,
frontend, or project CLI wrapper was changed by this slice.

## Implemented in source, still requiring independent review

The CLI exposes brands, channels, drafts, one post, queue, history, attempts,
worker health, draft create/update, explicit queue/exact scheduling and media
upload. It uses scoped bearer HTTP requests, disallows redirects and off-origin
escape-hatch paths, supports protected token files, bounds input/output sizes,
and never retries a failed write. MCP includes initialize/initialized, ping,
tools/list and tools/call; its upload root is opened at startup and file paths
cannot follow symlinks. `crates/cli/tests/integration.py` exercises the real
isolated API and MCP subprocess, including scope rejection, lost write response,
readback, upload confinement, redirect refusal and protocol errors. These
changes still require fresh independent review and integration.

The image recipe builds web/API/worker/CLI, includes ffprobe through FFmpeg, and
runs as UID 10001. Compose defines persistent DB/media and loopback API access.
Setup generates private unique deployment values and refuses existing projects.
Backup stops API/worker for a DB/media boundary and records a key fingerprint.
These files have not been built or rehearsed together.

## Explicit unfinished requirements

- Worker per-instance healthcheck and ID coordination. The Compose worker check
  intentionally exits 1; `up --wait` must not falsely report a healthy deployment.
  Do not replace it with `beweft-worker --healthcheck` before implementing that
  flag: the current binary ignores arguments and would start a second scheduler.
- Safe archive validation, restore into an explicit empty destination, matching
  encryption key checks, upgrade command and rollback documentation.
- Isolated Docker fresh install/restart/backup/restore/upgrade rehearsal including
  owner access, byte-identical media, drafts, durable queue and no live publishing.
- Production/TLS placeholder instructions, agent configuration and complete
  self-hosting documentation; setup currently has only concise command guidance.
- Dependency copyright/license notices in image and FFmpeg redistribution/source
  obligations. The entire image must not be represented as MIT.
- Project wrapper commands and fresh independent review of all new code.

The official MCP transport/lifecycle/tool specifications linked in
`operations-requirements.md` were consulted. No production deployment, Docker
app build, Compose project launch, backup or restore occurred in this slice.

Checks completed before pause: `cargo check -p beweft-cli`,
`cargo test -p beweft-cli` (one URL confinement regression), CLI Clippy with
`-D warnings`, CLI formatting check and POSIX shell syntax checks all passed.
The temporary Rust/Node image-version probes exited and their `--rm` containers
were removed. No task-created background process or test installation remains.
Build output in the shared `target/` is retained for unfinished work.
