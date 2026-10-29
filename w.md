# beweft project notes

Status: the owner resumed the full autonomous build on 2026-10-29 with three
implementation workers and the orchestrator. Current assignments, integration
decisions and verification boundaries are in
[the active delivery record](docs/orchestration-current.md).
Brand/session fixes from PRs #9 and #11, CLI/MCP qualification from PR #10,
named-worker health from PR #13, and provider removal fencing from PR #12 are
merged into the integration branch. Scheduling request recovery, media editing,
idea organization and self-host delivery are active checkpoints.
Results and workflow improvements: [cycle 1](docs/orchestration-cycle-1.md).
Start continuation with [the active delivery record](docs/orchestration-current.md);
[ORCHESTRATOR-HANDOFF.md](ORCHESTRATOR-HANDOFF.md) preserves the original launch
requirements and historical stack. Research decisions are
in `docs/research/` and `docs/architecture.md`. REA is installed for local Codex;
the authorized passive Buffer inventory is recorded in `docs/research/buffer.md`.
Provider implementation and
fixture verification do not establish live account access or publication.

Owner decisions: unicorn mascot and feel; self-host deployment uses placeholders
until a server/domain is selected. No production deployment target is approved.
Machine-specific browser configuration and approvals stay in the ignored local handoff.
Repository: https://github.com/scwlkr/beweft (already public; admin access verified).

Integrated: research/standards PR #2 and durable API/worker PR #3. Draft PRs #4–#7
preserve the web, workflows, providers and CLI/operations checkpoints. They are
not all ready to merge. See `docs/delivery.md` for the unfinished delivery work.
`docs/operations-requirements.md` defines installation and recovery acceptance.

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
