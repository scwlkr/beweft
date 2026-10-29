# beweft

beweft is an open-source, self-hosted social media manager in active development.
It gives one owner a workspace for multiple brands and their own social
connections, with an original unicorn identity.

The current build is paused at a documented continuation point. See the
[orchestrator handoff](ORCHESTRATOR-HANDOFF.md) for its PR stack, remaining work,
review findings and autonomous five-agent launch prompt.

The first application slice contains a Rust/Axum API, PostgreSQL storage, and a
separate durable publishing worker. It supports owner authentication, scoped
API tokens, brands, drafts, media, posting schedules, manual handoff, and
publication recovery. The full web interface, direct provider integration and
self-host packaging are being integrated; this is not yet a complete release.

No live social account or publication has been verified. Fixture results are
kept distinct from real provider outcomes. The [API contract](docs/api-contract.md)
and [provider research](docs/research/provider-capabilities.md) describe current
boundaries. Build the core with `cargo build --workspace`. Integration tests use
an isolated database; see `crates/server/tests/integration.py` for requirements.

The public-safe [build brief](HANDOFF.md) captures the intended scope. In the
owner's local project, `.local/HANDOFF.md` holds the complete approved prompt
for the build; this private file is not required by other users.
Project notes are in [w.md](w.md).
