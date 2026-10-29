# Delivery status

The full build remains active. Use [the current orchestration record](orchestration-current.md)
for source and installation checkpoints. The practical local release target is
2026-10-29 at 16:30 America/Chicago. Hosting remains local until the owner chooses
a server/domain. No live provider publication is authorized for tests.

## Reviewed and landed

The original PR stack #4–#8 and continuation milestones through #28 are merged
on main. These include multi-brand owner/scoped authentication, media and
metadata, drafts/channel variants, posting rules, queue/exact schedules,
immutable snapshots and receipts, worker recovery and heartbeat, ideas and
reusable templates, queued content replacement/priority, brand maintenance,
owner sessions/password, provider configuration/recovery, Rust CLI/MCP and
self-host operations. The current provider catalog and [setup guide](provider-setup.md)
name supported formats and limits; fixture results do not prove live access.

Fresh reviews covered the consequential corrections. Integrated checks include
real PostgreSQL, worker interruption/restart and multiple workers, scopes/CSRF,
media limits, populated migrations, scheduling/replacement uncertainty, owner
workflows, browser behavior, provider adapter/router fixtures and CLI/MCP.
The built baseline image passed normal local UI/API/worker use. A real
image/schema 7→14 upgrade and matching old-image/data/media/key rollback passed.
The combined image through #28 passed fresh independent qualification and was
installed backup-first before the 16:30 target. Its exact image/source and runtime
checks are recorded in the orchestration record.

## Remaining feasible product work

X threads, ordinary Facebook Page video, Instagram Stories eligibility,
Threads/Bluesky replies, LinkedIn organization opt-in, expanded Pinterest
formats/reads, Google Business Profile direct posting and Google grant-family
handling remain local implementation work. Bluesky entryway-to-PDS onboarding
and reconnect/queue migration are also unfinished. Do not classify missing code
as an external account-approval block or claim universal inbox/analytics support.

## External qualification

Provider app IDs/secrets, exact callbacks, scopes, account eligibility, costs,
consent and review are operator-specific. No live account or public post is
qualified by a fixture. Unsupported/private API use remains a manual handoff.
The owner chooses a public HTTPS origin and always-on host. PostgreSQL major
upgrades and physical host-reboot recovery remain unqualified.

## Nonblocking follow-ups

- #14: retain partial valid identity observations when another field makes a
  publication row malformed. The reproduction required later inconsistent API
  responses; no duplicate public post was demonstrated.
- #18: remove helper containers by inspected immutable ID to avoid deleting a
  deliberate same-name replacement.
- #21: include conservative video dimensions in the server error; the UI already
  displays those limits.
- #23: callers must validate receipt identity/intent when raw valid but incomplete
  JSON is returned; the encoded-query restriction also remains documented.
- #24/#28: strict Facebook completion evidence does not reject a contradictory
  optional upload phase. Public-stage reconciliation is owner-initiated and does
  not enforce a persisted polling throttle.
- #25: queued replacement checks media descriptions in the browser but lacks an
  atomic metadata-version precondition. A concurrent edit from another device
  after the check can change the frozen description shown in the comparison.
- #26: Bluesky video currently omits aspect ratio; playback can use a square card
  with letterboxing. Entryway/PDS migration remains unfinished as stated above.

Clean owned disposable fixtures and completed worktrees after use. Preserve
active work, installation data/keys, unique history and necessary rollback
material; never bypass Codex worktree ownership guards.
