# Active delivery record

The owner resumed the full build on 2026-10-29. This record supersedes paused
status in the historical handoff, not the requirements or unresolved findings.
It describes work in progress; it is not a release or live-provider claim.

## Integration

Verified starting integration branch: `codex/orchestrator-handoff`,
`f442efafbc69fda4e86369dd76244b4795cc10c3`. Main remains at the durable-core
delivery. Open PRs #4–#8 preserve the original stack; #9 preserves the bounded
brand-recovery trial. No implementation changes were discarded at resumption.

Reviewed fixes land into their explicit stack branches. The final stack will
land through a merge commit after integrated checks and fresh review, preserving
ancestor commits and PR history. Do not squash a parent beneath open descendants.
The owner authorizes merges without a routine human gate; automated checks and
brand-new independent reviews remain required.

## First assignments

All workers and reviewers use fresh contexts with `gpt-6.1-terra` at `max`.
There are at most three active children; a completed worker releases capacity
for a new read-only reviewer. Implementers never serve as their own independent
reviewer. Each milestone freezes a committed SHA and returns a compact report.

| Lane | Branch | First acceptance boundary |
| --- | --- | --- |
| Core/providers A+B | `codex/core-provider-recovery` | Remote revocation fences stale OAuth candidates and concurrent reconnect completion; deterministic blocked-revoke test. |
| Web C | `codex/web-recovery-delivery` | Authoritative overlapping brand snapshots, durable scheduling response-loss recovery and related session races. Based on preserved PR #9. |
| CLI/operations D+E | `codex/cli-operations-delivery` | Real API/CLI/MCP subprocess qualification and scoped file/token access; then installation/recovery rehearsal. |

Core owns server routes, worker behavior, provider modules, migrations and
generated API exports. Web owns client code and browser tests, except generated
contracts. CLI/operations owns agent interfaces, packaging and operator scripts.
The orchestrator owns integration, delivery status and cross-lane decisions.

Migration `0008` is reserved for revocation fencing. Core and web must agree a
durable schedule-request contract: the existing revision/channel unique index
excludes canceled publications and cannot alone make request replay safe.
The worker health interface must check a named instance's fresh heartbeat and
exit without starting a scheduler; core implements it and operations wires it.

## Runtime and resources

The existing local API at `http://localhost:3000/healthz` returned `status: ok`
at resumption. It is the retained earlier preview, not a verified current build.
The existing project PostgreSQL container remains available on loopback 55439.
Private configuration stays in ignored `.local` files. Other projects' services
and the user's browsers are outside cleanup ownership.

Workers use managed worktrees and owned disposable test databases/directories.
Mutable build output is isolated when concurrent builds could change binaries.
Retain only resources needed for unfinished work or review, then clean them and
retire completed managed worktrees through Codex. The orchestrator owns final
cleanup verification and the useful running installation.

No new provider connection, public post, paid call or Buffer queue mutation is
authorized for testing. Hosting stays local with server/domain placeholders.
Missing adapter code remains implementation work, not an external blocker.
