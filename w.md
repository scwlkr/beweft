# beweft project notes

Status: the full build is active. The reviewed application stack through PR #23
has landed on `main`, preserving its history. A healthy local Docker installation
is running on loopback port 3000 with first-owner setup available. Current work,
release qualification, remaining formats and review findings are in
[the active delivery record](docs/orchestration-current.md).

The practical local release target is 2026-10-29 at 16:30 America/Chicago. Core
UI/API/worker use and a real schema-changing image upgrade/rollback passed;
provider fixtures do not establish live account access or publication. Content/owner
UI and Meta, Bluesky and LinkedIn media milestones are merged through #28; the
combined installation image is being packaged and qualified.
The full intended product scope stays open until its feasible work is finished.

Owner decisions: original unicorn identity; self-host deployment uses placeholders
until a server/domain is selected. No production deployment target is approved.
Machine-specific browser configuration and private setup material stay ignored.
Repository: https://github.com/scwlkr/beweft (public; admin access verified).
Historical launch requirements and stack are preserved in
[ORCHESTRATOR-HANDOFF.md](ORCHESTRATOR-HANDOFF.md); current status is the active
record. Research is in `docs/research/`; [delivery.md](docs/delivery.md) records
remaining requirements and known limitations. REA is installed; the authorized
passive Buffer inventory is in `docs/research/buffer.md`.

The [build brief](HANDOFF.md) describes the intended product. A local private
handoff, when present, contains the owner's complete request for a build chat.
beweft is a general-purpose, open-source, self-hosted social media manager.
Preserve existing repositories, records, provider queues, and services.

Preferred stack: Rust/Axum, PostgreSQL, OpenAPI and Zod; React, TypeScript,
Tailwind and shadcn/ui; Cargo, pnpm and Docker Compose. Follow the installed
Wstack skill's current technical and review instructions.

<!-- wstack:begin setup -->
Wstack: 0.2.0
Features: [FEATURES.md](FEATURES.md)
Terms: [GLOSSARY.md](GLOSSARY.md)
CLI: `./project --help`
<!-- wstack:end setup -->
