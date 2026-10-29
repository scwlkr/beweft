# beweft

beweft is an open-source, self-hosted social media manager in active development.
It gives one owner a workspace for multiple brands and their own social
connections, with an original unicorn identity.

The project is in active development. See the
[delivery record](docs/orchestration-current.md) and
[orchestrator handoff](ORCHESTRATOR-HANDOFF.md) for the current work and review
boundaries.

The first application slice contains a Rust/Axum API, PostgreSQL storage, and a
separate durable publishing worker. It supports owner authentication, scoped
API tokens, brands, drafts, media, posting schedules, manual handoff, and
publication recovery. The full web interface, direct provider integration and
self-host packaging are being integrated; this is not yet a complete release.

To build a local self-hosted installation, see the
[installation, backup, restore and upgrade guide](docs/self-hosting.md). It uses
Docker Compose, keeps credentials in a private installation directory, and does
not require a public hostname or social account. The application source is MIT;
the image also contains third-party GPL/LGPL components with separate
[redistribution requirements](docs/image-licenses.md).

No live social account or publication has been verified. Fixture results are
kept distinct from real provider outcomes. The [API contract](docs/api-contract.md)
and [provider research](docs/research/provider-capabilities.md) describe current
boundaries. Build the core with `cargo build --workspace`. Integration tests use
an isolated database; see `crates/server/tests/integration.py` for requirements.
The [CLI and MCP guide](docs/cli.md) covers scoped tokens, command inputs and
the separate isolated CLI/MCP integration check.

The public-safe [build brief](HANDOFF.md) captures the intended scope. In the
owner's local project, `.local/HANDOFF.md` holds the complete approved prompt
for the build; this private file is not required by other users.
Project notes are in [w.md](w.md).
