# beweft project notes

Status: feasible local Buffer-parity work through PR #48 has landed on `main`
(`e831a28`). A healthy local Docker installation is running on loopback port
3000 with first-owner setup available. Current delivery status, residual nits
and review findings are in
[the active delivery record](docs/orchestration-current.md).

The practical local release checkpoint passed at about 16:17 America/Chicago
on 2026-10-29. Core UI/API/worker use and a real schema-changing image
upgrade/rollback passed; provider fixtures do not establish live account access
or publication. The combined installation image `beweft:release-bb359e3` remains
the qualified local image baseline. Live provider qualification and public
HTTPS hosting remain open owner decisions.

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
