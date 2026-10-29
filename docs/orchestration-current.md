# Active delivery record

The owner resumed the full build on 2026-10-29. This record supersedes paused
status in the historical handoff. Work remains in progress; no live-provider
qualification or completed release is claimed.

## Integration

Integration branch: `codex/orchestrator-handoff`, provider-wiring merge `f2ad5a5`.
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
  Production provider routes subsequently landed in PR #20. The browser warning
  and recovery controls still need the new production contracts.

- PR #16, idea organization: `9bcc379`, reviewed head `a770e43`. Real API
  browser scenarios cover board stages, groups, tags including literal `all`,
  deletion, outages, mobile layout and delayed actions across brand changes.
  Full screen-reader and keyboard qualification was not separately performed.

## Active contracts and checkpoints

| Lane | Checkpoint | Next acceptance boundary |
| --- | --- | --- |
| Core/providers A+B | PR #20 merged `f2ad5a5`; X video milestone started | Current v2 streaming upload, durable stages and uncertainty handling; thread media needs an explicit snapshot contract afterward. |
| Web C | PR #14 at `a22550d`, fresh review in progress; #15 and #16 merged | Previous review found discarded partial IDs and preference recovery/fencing issues. The correction candidate also replaces a timing-sensitive aggregate owner-test assertion with per-request checks. Preserved manual guidance awaits integration. |
| CLI/operations D+E | PR #18 correction after `f65402d` | Recovery, controlled state and subprocess supervision passed targeted review. The remaining initializer helper needs a durable operation identity and exact cleanup across interruption. |

PR #20 now wires the production provider routes and worker, durable terminal
schedule rejection/retirement receipts, and reply revision/active-executor fences.
Fresh independent review at `b40eed8` found no actionable issues after the
capability/input-validation corrections. Default/fixture startup preflights,
production router/worker fixtures and independent scope/reconnect variations
passed in the loopback sandbox. It merged at `f2ad5a5`. A lost database session does not prove
an outbound sender stopped: owner resolution requires a truthful sender-stopped
attestation as well as the observed reply revision.

PR #18's review exercised nonempty backup/restore, owner access, media, drafts,
queue, post-restore upload, activation holds, concurrent MCP/log usage and signal
handling. These results do not yet qualify a schema-changing application upgrade.
The retained old application image is reserved for that later integrated rehearsal.

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
without changing stored draft JSON or historical publication payloads. It merged at `6e57386` after #15; the local merge tree was verified exactly.
Client adoption is still unfinished. The contract prevents an unknown schedule after reload from silently
snapshotting later description edits.

Media saves persist owner/brand/asset/base-version/text before PATCH. Unknown
outcomes stay unresolved; same-CAS retry is explicit. A read at the base version
cannot prove a pending write ended. A later version fences the old write; UI
cache reconciliation must be monotonic and status accessible in the dialog.

## Runtime and resources

The old preview processes are no longer running and port 3000 has no listener
at the latest check. Its existing dev database remains intact: one owner, brand,
channel, draft, media item and publication; no provider apps, migrations 1–2.
These records/media are preserved. The final local installation is still pending.
The project PostgreSQL container stays on loopback 55439; other services and
normal browsers are protected.

At merged `6e57386`, the root workspace build, web build, both scheduling API
suites and all 93 isolated Chromium scenarios passed. Temporary fixture resources
were cleaned by the harnesses. At `f2ad5a5`, the root workspace and web builds plus both scheduling API
suites passed again. The full server integration suite also passed, including
restart/two-worker recovery, scoped authentication, workflow operations, password
rotation races, streamed uploads and populated-schema migration. This does not
replace the final image/schema upgrade rehearsal. Root binaries and web/dist now match that provider-wiring
merge; they are not a running preview.

During provider #20 qualification, a shared-target feature mismatch may have
sent synthetic OAuth client/code data to Facebook's token endpoint. No real
credentials or public content were involved. That run stopped. Startup guards
now reject mismatched fixture builds/origins, and provider fixture runners use a
verified loopback-only OS sandbox. Subsequent sandboxed runs passed; no live
provider qualification is claimed. Public posts, paid calls and Buffer queue
changes remain outside testing authorization.

Workers own isolated test databases/processes/artifacts. Shared mutable outputs
are avoided; retained builds are tied to a source SHA. Finished review worktrees
are retired through Codex. Eleven merged local branches and ten merged remote
branches were removed after checking ancestry; active and stacked PR branches
remain protected. A research branch with unique history was also retained.
The orchestrator owns final cleanup and a useful
running local installation. Hosting remains placeholders until the owner chooses
a server/domain. Missing adapter code is unfinished work, not an external blocker.

## Remaining delivery sequence

After the active corrections pass fresh independent review, integrate their frozen
heads and continue practical provider formats, provider connection/recovery UI,
templates and queue maintenance, brand/security settings and CLI/MCP receipt
workflows. New nested media formats must participate in the complete frozen media
union/version contract. Adding Google Business Profile must account for shared
Google app/grant revocation across YouTube and Business Profile.

Final acceptance includes integrated default-build and isolated provider checks,
a fresh useful local installation, restart/recovery, a real schema-changing
application upgrade and matching backup restore/rollback rehearsal. Preserve the
old dev data. Merge the original PR stack with its ancestry intact, keep capability
and operator docs current, then retire owned disposable resources. No physical
host reboot, live account qualification or public provider publication is claimed.
