# Self-host delivery requirements

This is the packaging acceptance contract while implementation is in progress.
No production server or hostname has been selected. Examples use placeholders;
local operation must not require public DNS or access to a social provider.

## Services and storage

- One application image supplies the API and worker as separate Compose services.
  Include the built web UI and media inspector. Build without provider fixture
  features; run as an unprivileged user.
- PostgreSQL and media use persistent volumes. Only the API's chosen loopback
  port is published by default; PostgreSQL stays on the internal network.
- API and worker restart after host reboot. API database readiness and worker
  heartbeat are separate health signals; a working login page is insufficient.
  Each worker sharing a database needs a unique, stable `BEWEFT_WORKER_ID`;
  the default Compose worker uses `compose-primary` and is not a scalable ID
  template for additional replicas.
- A reverse proxy supplies HTTPS in production. Operators set the actual public
  origin before OAuth; example hostnames are never treated as deployment targets.

## First use and secrets

- Setup generates unique database, owner-bootstrap and encryption secrets into
  ignored files with restrictive permissions. No shared example password or
  development encryption key may become a deployment default.
- Setup must refuse to overwrite existing secrets or reset a database. Re-running
  setup should identify the existing installation and explain the next step.
- Preserve the encryption key across restarts and upgrades. Provider secrets stay
  server-side. Backup instructions must cover key recovery explicitly.
- Fixtures are disabled. First sign-in creates the owner through the guarded
  bootstrap flow; demo content and simulated channels are not automatically added.

## Backup, recovery and upgrades

- Document a consistent backup boundary for PostgreSQL and media, including how
  to quiesce API mutations and stop worker claims. Bound downtime and explain
  uncertain in-flight publications after interruption.
- Keep credentials separate from shareable archives. Restore instructions check
  the matching encryption key, archive paths and explicit destination before
  replacing data. A backup never proves recovery until restored and exercised.
- Verify backup/restore against isolated volumes/database: owner access, media
  byte identity, draft content and queue state survive. Never publish during the
  rehearsal. Delete disposable rehearsal resources afterward.
- Upgrades record the old image/revision and require a backup before migrations.
  Do not promise automatic schema rollback. Describe restoring the matching
  database/media/key with the old application if rollback is necessary.

## Agent access

The Rust CLI and stdio MCP server use scoped API tokens and the same HTTP actions
as the UI. They must support inspecting brands/channels/drafts/queue/outcomes,
creating and updating drafts, scheduling with explicit intent, media upload and
checking worker health. MCP tools clearly distinguish reads from writes; draft
creation must never schedule implicitly. Unknown writes remain recoverable and
must not be silently retried. Tokens are read from environment/protected files,
never printed in normal diagnostics.

For stdio MCP, follow the official [transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports),
[lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)
and [tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
contracts: newline-delimited JSON-RPC, protocol-only stdout, initialization and
capability negotiation, discoverable schemas, and explicit tool errors. Exercise
the subprocess protocol against the real isolated API, including scoped-token
rejection and a write followed by reading its authoritative result.

## Delivery evidence

Rehearse a fresh local installation and upgrade, durable worker scheduling across
restart, backup/restore and CLI/MCP read/write behavior with isolated fixtures.
Report checks in chat and keep only maintained test harnesses in the repository.
The final running local app remains useful without live provider credentials;
provider consent/review and the eventual server/domain are separate next steps.
