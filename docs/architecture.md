# Architecture and product decisions

Beweft is an owner-operated social workspace with multiple brands. The original
identity is a unicorn mascot with restrained spectrum accents, ivory/lavender
surfaces, and equally deliberate dark colors. Personality belongs in onboarding
and empty states; operational errors use precise language.

## Chosen approach

Use a Rust workspace: Axum API, separate publishing worker, PostgreSQL, and a
Rust HTTP client CLI/MCP. React, TypeScript, Tailwind and shadcn/ui provide the web
interface. OpenAPI documents the server contract; Zod validates browser boundaries.
The UI, CLI and MCP use the same authoritative HTTP actions.

A single combined process is simpler but couples worker availability to API
restarts. Separate services in one codebase preserve the operational boundary
without adding microservices, Redis, or another publishing backend. This choice
was challenged by a separate architecture critic before implementation.

Brand-scoped navigation keeps cross-channel planning together. A channel-first
view remains available through filters. The composer expands on desktop and
becomes a full-screen workflow on phones.

## Correctness requirements

- Separate post content from each channel's publication outcome.
- Freeze a content revision when scheduling; reject conflicting edits with
  actionable feedback.
- Reserve queue slots transactionally in the channel's IANA timezone. Treat exact
  schedules separately and define daylight-saving behavior.
- Persist the sending attempt before the provider request. Expired sending work
  becomes uncertain and requires reconciliation, never an automatic fresh send.
- Keep provider receipts, definite failures, uncertain outcomes and owner-marked
  manual completion distinct. Do not promise universal exactly-once delivery.
- Track implementation, configuration, permissions, execution method and test
  evidence separately. Fixture success is not verified platform access.
- Protect owner bootstrap, session cookies, scoped API tokens, OAuth state and
  PKCE; encrypt provider secrets on the server with a separately recoverable key.
- Persist media independently of containers. Backups require database, media and
  secrets; restore verification is part of delivery.

## Delivery sequence

1. Research and repository standards; establish actual access and limits.
2. Functional authenticated brand/media/draft/schedule/publication vertical slice.
3. Direct adapters and bring-your-own app setup; provider-specific validation.
4. Polished complete web workflows, analytics/inbox where supported, CLI/MCP.
5. Real database/browser/worker recovery checks, independent review and fixes.
6. Compose packaging, backup/restore/upgrade exercise and deployment where a
   confirmed isolated target is available.

Each meaningful slice is checked, independently reviewed in fresh context,
committed, pushed and merged. This sequence is not a reduction in the approved
scope. Provider approvals and live credentials are external qualification steps;
they must not hide unfinished local work.
