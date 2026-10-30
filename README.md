# beweft

beweft is an open-source, self-hosted social media manager in active development.
It gives one owner a workspace for multiple brands and their own social
connections, with an original unicorn identity.

The project is in active development. See the
[delivery record](docs/orchestration-current.md) and
[orchestrator handoff](ORCHESTRATOR-HANDOFF.md) for the current work and review
boundaries.

The reviewed local application includes a React web workspace, Rust/Axum API,
PostgreSQL storage, durable publishing worker, and scoped Rust CLI/MCP. It
supports owner authentication, multiple brands, ideas/templates, drafts/channel
variants, media descriptions, queue and exact schedules, manual handoff,
publication recovery, brand and owner security controls, and self-host operations.
The combined local image has passed independent workflow and restart checks.
Additional provider formats remain in development; see the delivery record for
the precise remaining scope.
To build a local self-hosted installation, see the
[installation, backup, restore and upgrade guide](docs/self-hosting.md). It uses
Docker Compose, keeps credentials in a private installation directory, and does
not require a public hostname or social account. The application source is MIT;
the image also contains third-party GPL/LGPL components with separate
[redistribution requirements](docs/image-licenses.md).

For a separate local workspace populated with example content, see the
[demo setup](docs/self-hosting.md#example-workspace). It starts with manual
practice channels and does not connect a social account.

## Local setup

From a checkout:

```sh
./project setup .local/beweft beweft 3000
./project build .local/beweft
./project compose .local/beweft up -d --wait --no-build --pull never
```

Open `http://127.0.0.1:3000`, create the owner with the setup secret from
`.local/beweft/installation.env`, and choose an owner password. For an isolated
example workspace:

```sh
./project demo create .local/demo beweft-demo 3001 <trusted-local-image>
./project demo resume .local/demo
```

Run `./project --help` for the complete command list. Backup, restore, upgrade
and recovery steps are in [self-hosting.md](docs/self-hosting.md).

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
