# Delivery checklist

Implementation resumed on 2026-10-29; use
`docs/orchestration-current.md` for active checkpoints and
`ORCHESTRATOR-HANDOFF.md` for the starting stack and requirements.
This is working status, not a release claim. Hosting remains local until the
owner chooses a server and domain. Example production values must stay clearly
marked placeholders. No live provider publication has been authorized for tests.

The bounded cycle-1 trial is recorded in `docs/orchestration-cycle-1.md`.
Its brand-list and browser-session follow-ups were subsequently resolved in
PR #11 and integrated through PR #9. CLI/MCP qualification (#10), named-worker
health (#13) and provider removal fencing (#12) also landed in the integration
branch. Main still awaits the full stack's integrated delivery checks.

## Complete

- Public repository verified; research and contribution standards merged in PR #2.
- Buffer UI inventory plus bounded REA passive inspection; source comparison of
  Postiz, OpenPost and Mixpost; official provider access constraints researched.
- Separate architecture critique and unicorn identity decision.
- Durable core merged in PR #3 after fresh review: owner/scoped authentication,
  transactional versioned scheduling, attempt fencing and recovery, streamed
  quota-limited media, and generated OpenAPI/TypeScript contracts. Isolated
  integration checks include real worker interruption, two workers, late-worker
  completion, pause races, DST and populated-schema upgrade.

## Implemented, under review/integration

- PostgreSQL/Axum owner authentication, scoped agent tokens and multiple brands.
- Drafts, channel variants, persistent media and weekly posting rules.
- Transactional scheduling, immutable content snapshots, queue operations,
  worker heartbeat, attempts, uncertain outcomes and manual handoff.
- React interface with responsive navigation, composer, queue/calendar, ideas,
  media, history, settings and appearance.
- Direct-provider crate, encrypted app/grant storage and OAuth destination flow.
- Content-workflow backend passed fresh review: ideas/groups/tags, templates,
  per-media descriptions, manual-platform guidance, atomic queue replacement,
  priority, brand archive and owner session/password management. Integration
  checks include concurrent old-password login during password rotation.

## Still required before delivery

- Independent review and any consequential fixes with fresh review.
- Provider-to-worker integration, granular media/format validation, token refresh,
  disconnect/reconnect, receipts/reconciliation and provider fixtures.
- Real supported analytics/inbox reads and replies with capability/coverage
  reporting; no fabricated metrics or universal inbox promises.
- Useful Rust CLI/MCP consuming the authoritative HTTP API (partial checkpoint).
- Remaining practical workflow refinements: idea organization, reusable original
  templates, tags, media metadata, post duplication and clear timezone behavior.
- Production Compose packaging, persistent volumes, setup, backup/restore,
  upgrade and worker health documentation; local deployment rehearsal
  (partial checkpoint).
- End-to-end responsive checks, partial failures, restart/recovery, contract and
  adapter checks on the integrated build.
- README/features/glossary/current instructions, merged focused PRs, clean
  disposable artifacts, and a running local preview with usable instructions.

## External qualification

Provider app IDs/secrets, exact callbacks, approved scopes, account eligibility,
API costs and provider review are operator-specific. A fixture passing does not
establish live access. TikTok private/internal Direct Post eligibility and any
network without a supported public API require explicit manual preparation.

The final delivery must name unsupported actions precisely. A missing adapter
implementation is local unfinished work, not a provider-access blocker.
