# Active delivery record

The owner resumed the full build on 2026-10-29. This record supersedes paused
status in the historical handoff. Work remains in progress; no live-provider
qualification or completed release is claimed.

## Integration

Integration branch: `codex/orchestrator-handoff`, ideas merge `9bcc379`.
Main remains at durable-core PR #3. Open PRs #4–#8 preserve the original stack.
Reviewed additions land through merge commits; the completed stack will land
without squashing an ancestor beneath open descendants.

The owner authorizes autonomous merges. Automated checks and a brand-new
read-only reviewer for every consequential code revision remain required.
All workers/reviewers use fresh contexts with `gpt-6.1-terra` at `max`, within
three active children plus the orchestrator. Each milestone freezes its SHA.

## Integrated checkpoints

- PR #10, CLI/MCP qualification: `92761c9`. Real API and subprocess checks cover
  scoped access, upload handles, bounded cursor pages and large valid bodies.
  Percent-encoded query handling has a nonblocking restriction queued for the
  next CLI slice. Scheduling receipt recovery remains to add.
- PR #11 passed fresh review at `c45f2f2` and landed through preserved PR #9 at
  `5a42e6e`. Brand discovery/fallback ordering and browser-session boundaries
  are covered, including stale responses, direct 401s and delayed sign-out.
  The integrated build passed all 66 isolated Chromium scenarios at this merge.
- PR #13, named-worker health: `c8e7760`. The probe reads the configured worker's
  fresh heartbeat without starting a worker, migrations or scheduler. Isolated
  process/PostgreSQL, hostile environment and actual Docker-health checks passed.
- PR #12, provider removal fencing: `e90403e`, reviewed head `5cfe0bb`.
  Disconnect sends no direct-provider revocation request. A durable fence blocks
  reconnect until owner-attested access removal; same-provider siblings retain
  credentials/queues but conservatively require reconnection. Fixtures cover
  every direct provider, stale grants/callbacks, busy operations and pool pressure.
  The review's final sync-guidance finding was adjudicated nonblocking: recovery
  explicitly waits for active work, inspects connected channels and permits
  removal only of orphaned access. Disconnect itself waits for channel locks.
  Provider routes and the pre-disconnect warning remain to wire into production.

- PR #16, idea organization: `9bcc379`, reviewed head `a770e43`. Real API
  browser scenarios cover board stages, groups, tags including literal `all`,
  deletion, outages, mobile layout and delayed actions across brand changes.
  Full screen-reader and keyboard qualification was not separately performed.

## Active contracts and checkpoints

| Lane | Checkpoint | Next acceptance boundary |
| --- | --- | --- |
| Core/providers A+B | PR #19 metadata preconditions reviewed; provider wiring started | Integrate #15/#19, then qualify production routes and worker dispatch; practical formats still follow. |
| Web C | PR #14 scheduling; #15 media descriptions; #16 idea organization | PR #15 `7e62ad3` fixes known-key isolation and scoped namespace gaps; fresh review is underway. Ideas #16 is merged. Scheduling and preserved manual guidance await integration. |
| CLI/operations D+E | PR #18 recovery corrections | The second fresh review confirmed earlier fixes but found an unbounded downtime lookup, project ownership collision, unpinned PostgreSQL and helper-volume cleanup; these are being corrected. |

Core owns server/worker/providers/migrations/generated exports. Web owns client
and browser tests, plus a narrow loan for media metadata CAS and authorized GET:
migration 0010 adds `metadata_version`; PATCH requires a matching version and
increments it. Generated files receive a coordinated source-driven refresh.
Operations owns CLI/MCP, packaging and operator scripts. The orchestrator owns
contracts, integration, docs, delivery readiness and final cleanup.

New web scheduling writes persist a UUID and immutable normalized mutation.
Backend request replay precedes current-version/time checks and returns original
publication IDs even after cancellation/replacement. GET receipts return bounded
publication summaries. A missing receipt cannot prove an in-flight write ended.
PR #17 passed fresh independent review at `55eab7c` and merged at `6a351f0`.
Nonblocking admission leaves recovery/authentication reads usable under repeated
same-key retries. Compact summary display fields cap channel names at 100 and
provider labels at 64 Unicode characters, including for existing rows. Large
escaped/multibyte label fixtures without a media-version map keep receipts
  below 100 KB with original IDs. The 1,000-entry map boundary in PR #19
  measured 123,182 bytes; the contract does not impose a 100 KB ceiling.
Web and CLI consumers still require adoption; backend availability alone does
not close the browser scheduling uncertainty finding.
Timestamps accept at most six fractional digits and normalize exactly to UTC;
client comparison must preserve microseconds, not truncate through Date.parse.

The next schedule contract adds an optional normalized media-version map to the
immutable request and receipt. New clients will supply the complete effective
media union; the server will lock and check versions in the snapshot transaction.
PR #19 passed fresh independent review at `b126474`, including legacy UUID
spellings, populated migration, edit races, scoped receipts, complete rollback
and concurrent mismatch retries. Read responses normalize known UUID fields
without changing stored draft JSON or historical publication payloads. Its merge
awaits #15 and the inherited web dependency conflict; client adoption is still
unfinished. The contract prevents an unknown schedule after reload from silently
snapshotting later description edits.

Media saves persist owner/brand/asset/base-version/text before PATCH. Unknown
outcomes stay unresolved; same-CAS retry is explicit. A read at the base version
cannot prove a pending write ended. A later version fences the old write; UI
cache reconciliation must be monotonic and status accessible in the dialog.

## Runtime and resources

The retained preview at localhost:3000 is an earlier API/worker build, not the
current integrated release. Its served web bundle was rebuilt during checks.
The project PostgreSQL container remains on loopback 55439. Private settings
stay ignored. Other projects' services and normal browsers are protected.

Root workspace build and all 66 browser scenarios passed at `5a42e6e`. Root
binaries predate the worker-health/provider merges; broader integrated checks
follow the remaining contract and workflow merges. No live provider account,
public post, paid call or Buffer queue mutation was used.

Workers own isolated test databases/processes/artifacts. Shared mutable outputs
are avoided; retained builds are tied to a source SHA. Finished review worktrees
are retired through Codex. The orchestrator owns final cleanup and a useful
running local installation. Hosting remains placeholders until the owner chooses
a server/domain. Missing adapter code is unfinished work, not an external blocker.
