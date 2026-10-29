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
health (#13), provider removal fencing (#12), media descriptions (#15), idea
organization (#16), durable scheduling receipts (#17/#19), and production provider
wiring (#20) also landed in the integration branch. Main still awaits the full
stack's integrated delivery checks.

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
- Practical provider format expansion beyond the current limited adapters.
  Production routes, worker dispatch, scoped grants/refresh, durable receipts and
  reply/revocation fences landed in #20 after fresh sandboxed review; that does
  not establish live access. Provider connection/recovery UI still needs adoption.
- Real supported analytics/inbox reads and replies with capability/coverage
  reporting; no fabricated metrics or universal inbox promises.
- Useful Rust CLI/MCP consuming the authoritative HTTP API (partial checkpoint).
- Remaining practical workflow UI: reusable original templates, atomic queued
  editing/priority, brand archive/rename, owner sessions/password, useful provider
  metadata and manual guidance. Idea organization and media metadata are merged.
  Browser scheduling recovery remains under correction in #14.
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

## Practical release checkpoint

The local release target is 2026-10-29 at 16:30 America/Chicago. Full product
scope remains open: prioritize working owner workflows, common failures, safe
uncertain outcomes and a useful installation. Fresh review still blocks serious
practical defects; uncommon recoverable cases are recorded rather than used to
repeat broad hardening loops. A release checkpoint does not qualify untested
provider formats or complete unfinished functionality.

PR #14 merged at `d0681e4`, reviewed at `a22550d`. Its nonblocking P2 follow-up:
when one field makes a publication row malformed, contradictory valid fields
can be ignored. Reproductions required subsequent inconsistent/fabricated API
lookup and receipt responses; no duplicate public post was demonstrated. Common
lost-response recovery passed. Retain partial identity observations field by
field in a later narrow correction; do not claim every contradictory-response
combination is covered.

## Landed main and qualification update

The preserved PR stack #4–#8 landed on main at `a60b531`, with the same application
tree as independently reviewed `3b4445b`. The built image passed normal local
UI/API/worker use. A real image/schema 7→14 upgrade and old-image/database/media/key
restore passed, including subsequent upload and a held rollback worker. The
retained local installation was upgraded backup-first and remains ready for the
owner's first setup. This does not qualify live providers or finish the active
#24–#27 and remaining format work.

Additional nonblocking follow-ups: #18 helper cleanup can remove a deliberate
same-name replacement container (future cleanup should use the inspected immutable
ID); #21's server error omits its conservative video dimension limit (the UI shows
it); #23 returns structurally incomplete but valid JSON as raw data, so callers
must validate receipt identity/intent before treating an outcome as terminal.
The raw CLI percent-encoded query restriction remains documented. #24's strict
Facebook completion check does not reject a contradictory optional uploading
phase; its publishing/status evidence remains fixture-qualified only.
