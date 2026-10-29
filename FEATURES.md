# Features

The following behavior is reviewed and merged on main. The local image passed
independent UI/API/worker qualification; no live provider account or publication
is implied.

- **Publishing core:** Multiple brands, drafts/channel variants, persistent
  media/descriptions, weekly schedules, exact times, queue priority, pause,
  immutable snapshots and durable worker attempts.
- **Recovery:** Fenced completions, uncertain outcomes, durable scheduling
  receipts, audit-linked queued replacements, provider checkpoints and explicit
  owner-confirmed manual handoff.
- **Authentication/API:** Owner setup/sign-in, scoped agent tokens, session and
  password maintenance, OpenAPI and generated TypeScript contracts.
- **Web workspace:** Original unicorn identity, responsive composer,
  queue/calendar, ideas/groups/tags, templates, media, brand/owner settings,
  provider connections, manual guidance and supported recovery controls.
- **Direct connections:** Owner-supplied apps, encrypted grants, OAuth destination
  selection, scoped publishing, supported observations/replies, and conservative
  provider access-removal fences. The running provider catalog names actual
  formats and read/reply capabilities; account approval is separate.
- **Agent/operations:** Rust CLI/MCP, Docker Compose with persistent data,
  first-owner setup, consistent backup/restore, worker-held recovery, immutable
  image pinning and upgrade/rollback workflows.

Further formats and onboarding work are listed in [delivery status](docs/delivery.md).
Use [provider setup](docs/provider-setup.md) for current limits and
[official-source research](docs/research/provider-capabilities.md) for network
constraints. Fixture success is distinct from live qualification.
