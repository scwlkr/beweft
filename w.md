# beweft project notes

Status: full implementation in progress. Research and architecture decisions are
in `docs/research/` and `docs/architecture.md`. REA is installed for local Codex;
authorized passive Buffer investigation has begun. Provider implementation and
fixture verification do not establish live account access or publication.

Owner decisions: unicorn mascot and feel; self-host deployment uses placeholders
until a server/domain is selected. No production deployment target is approved.
Existing Chrome remote debugging was enabled with explicit permanent approval.
Repository: https://github.com/scwlkr/beweft (already public; admin access verified).

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
