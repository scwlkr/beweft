# Features

Implementation is underway. This inventory distinguishes merged behavior from
work still being integrated; it is not a live-provider qualification claim.

- **Durable publishing core (merged):** Multiple brands, drafts and channel
  variants, persistent media, weekly schedules, exact times, queue ordering,
  pause, immutable publication snapshots and a separate publishing worker.
- **Recovery (merged):** Durable attempts, expired-worker uncertainty, fenced
  completions, explicit reconciliation and owner-confirmed manual handoff.
- **Authentication and API (merged):** Owner setup/sign-in, brand-scoped agent
  tokens, OpenAPI and generated TypeScript request/response types.
- **Web workspace (under review):** Original unicorn identity, responsive
  composer, queue/calendar, ideas, media, connection settings and appearance.
- **Direct connections (under review):** Owner-supplied provider apps, encrypted
  grants, OAuth destination selection, refresh, observations and replies.
  Router/worker integration and further format coverage remain in progress.
- **Content organization (in progress):** Idea groups/status/tags, templates,
  media descriptions, atomic queued-content replacement and brand maintenance.
- **Agent and operations delivery (pending):** Rust CLI/MCP, Docker Compose,
  backup/restore and upgrade rehearsal.

See [provider research](docs/research/provider-capabilities.md) for network
constraints. No real provider account or publication has been qualified.
