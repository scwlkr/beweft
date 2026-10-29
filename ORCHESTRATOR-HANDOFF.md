# beweft: autonomous orchestrator handoff

Prepared 2026-10-29 at the owner's explicit request to pause implementation and
transfer the remaining build to an orchestrator with five parallel agents.
This is a continuation, not a fresh scaffold. No implementation agents remain
running from the previous session. Read the launch prompt below in a new chat.

## Goal and authority

Finish beweft as a polished, original, open-source, self-hosted social manager
for one owner with multiple brands. Buffer is the usability benchmark, not a
dependency. The personal identity is an original unicorn mascot with restrained
lavender/ivory/spectrum accents and deliberate dark mode. Preserve that identity.

The owner explicitly authorizes autonomous commits, pushes, pulls, PR creation
and merges without human verification or routine approval. This overrides
Wstack's routine human release/merge gate for this project. Automated checks and
fresh independent code reviews remain required. Do not turn this into permission
requests at each milestone or stop after an MVP.

Existing boundaries still apply: no spending, public test posts, changes to live
Buffer queues, interference with Bubba's services, or guessed production domain.
The owner selected **hosting placeholders for now**. Finish local/self-hosted
delivery; a production server/domain remains an external choice. Provider
credentials, approval and live qualification cannot be invented.

## Source of truth

- Repository: <https://github.com/scwlkr/beweft>, public; verified remote `origin`.
- Start with `AGENTS.md`, `w.md`, `HANDOFF.md`, and `.local/HANDOFF.md` if present.
- Read the installed Wstack skill and its technical-stack/code-review references.
- Research: `docs/research/{buffer,connections,provider-capabilities,rea}.md`.
- Architecture: `docs/architecture.md`; API: `docs/api-contract.md`,
  `docs/workflows-api.md`, generated `docs/openapi.json`.
- UI status: `docs/frontend.md`; operations status:
  `docs/operations-implementation-status.md` and `docs/operations-requirements.md`.
- Local-only process/runtime details: `.local/ORCHESTRATOR-LOCAL.md`, when present.
  Never commit `.local`, credentials, captures or private account content.

## Preserved PR stack

Main currently includes research/standards [PR #2](https://github.com/scwlkr/beweft/pull/2)
and the durable API/worker [PR #3](https://github.com/scwlkr/beweft/pull/3).
Main's verified merge is `26e3ebf45fb530c5d004f04216bb94d24ddf54e6`.

| PR | Branch / checkpoint | Status and next action |
| --- | --- | --- |
| [#4](https://github.com/scwlkr/beweft/pull/4) | `codex/web-workspace`, `ef3120a` | Draft. Responsive UI and several reviewed fixes; scheduling uncertainty fix is unfinished. Brand fallback fix needs its regression and fresh review. |
| [#5](https://github.com/scwlkr/beweft/pull/5) | `codex/content-workflows`, `142c2ab` | Draft, based on #4. Backend workflow slice passed fresh independent review. Frontend controls remain to implement. |
| [#6](https://github.com/scwlkr/beweft/pull/6) | `codex/provider-adapters`, `ae56dec` | Draft, based on #5. Provider modules and fixtures; one outstanding revocation/reconnect race. Routes/worker are not wired. |
| [#7](https://github.com/scwlkr/beweft/pull/7) | `codex/cli-self-hosting`, `d0fd324` | Draft, based on #6. Compiling partial CLI/MCP and unverified packaging. See its explicit status document. |

The handoff branch `codex/orchestrator-handoff` contains this entire stack plus
the handoff documentation. For a fresh clone, fetch and check out that branch
after confirming there is no unrelated local work. Inspect remote state before acting; do not
discard unique commits or assume every draft is ready to merge. Use merge
commits while integrating this stack, or deliberately rebase all descendants
and retarget their PRs together. Do not squash a parent and leave descendants
with duplicate historical diffs. Never force-push another agent's active branch.

## Exact unresolved review findings

1. **UI scheduling uncertainty — P1, not fixed.** `web/src/useComposer.ts`
   saves/PATCHes before scheduling. If scheduling commits but its response is
   lost, retrying creates another revision and another publication. Backend
   revision/channel uniqueness only protects the original revision. New
   `scheduleIntent.ts` and `usePendingSchedule.ts` are compiling helpers but are
   **not wired into the composer**. Finish immutable intent persistence,
   authoritative outcome recovery, close/reload behavior and an explicit safe
   retry. Do not PATCH to a new revision while an outcome is unknown. Test a real
   committed schedule with its response lost and prove only one publication.
2. **Automatic brand fallback — implementation awaiting verification.**
   `useWorkspace.ts` now uses the same cleanup/invalidation for fallback as for
   explicit selection. Add the removed/archived-active-brand plus failed-next-
   brand-load regression. No stale draft or composer may remain editable.
3. **Disconnect/reconnect versus remote revocation — P2, not fixed.**
   `connections/credentials.rs` commits disconnect and releases its lock before
   remote revoke. An already-authorized reconnect can complete during the gap;
   delayed Google revocation invalidates its fresh grant while local state says
   connected. Fence revocation generations and reject candidates authorized
   before that boundary. Test a blocked revoke, concurrent reconnect completion,
   then released revoke. [Google's revocation behavior](https://developers.google.com/identity/protocols/oauth2/web-server#tokenrevoke)
   matters here; a channel lock covering only local writes is insufficient.

Each consequential fix requires a **brand-new, read-only reviewer with no
implementation conversation**. Prior reviewers cannot review their own fixes.

## Implemented and verified boundaries

- Core: owner/bootstrap authentication, scoped brand tokens, persistent media
  quotas/streaming, draft/channel variants, posting rules, queue/exact schedules,
  immutable snapshots, attempts, worker heartbeat and uncertain/manual recovery.
- Core tests include real PostgreSQL, worker interruption/restart, multiple
  workers, stale-attempt completion, pause races, DST, media quota/cancellation,
  typed schemas and populated migration. These passed before the handoff.
- Workflow backend: idea status/groups/tags, original templates, frozen media
  descriptions, manual guidance, archive, queued-content replacement/priority,
  password/session maintenance. Full tests passed; password rotation received
  a deterministic old-password-login race regression and fresh passing review.
- Web: responsive unicorn workspace, themes, composer, queue/calendar, ideas,
  media, schedules, history/recovery, connections, analytics/inbox states and
  owner settings. Twelve browser scenarios passed before the last partial
  handoff fix. Latest checkpoint passes build/TypeScript; **the new fallback and
  scheduling-intent changes have not passed fresh E2E/review**.
- Provider fixtures: OAuth replay/session/config binding, refresh concurrency,
  cross-origin rejection, private reply audiences, reply/disconnect races,
  history watermark, expired upload reset, stale worker zero-write fencing,
  committed media capability fetch during provider POST, rotating credentials
  surviving downstream failure, and actual reply-claim deadlines. Five adapter
  tests and the PostgreSQL/HTTP suite passed at the last provider checkpoint.
  Latest review was static and found the revocation race above.
- CLI: compiles and passes its small URL-confinement test/Clippy. No real
  API/MCP subprocess qualification or fresh independent review yet.
- No real provider account, paid request or live publication has been verified.
  Browser provider fixtures prove UI behavior; they are not live evidence.

## Remaining product work

1. Resolve the review findings and integrate the stack in small checked units.
2. Wire direct-provider routes, catalog, callbacks/media capability routes,
   analytics/inbox and worker dispatch into the server. Pass attempt identity
   through every publication stage. Preserve archive/scope/lease safeguards.
   Regenerate OpenAPI and TypeScript from the Rust contract.
3. Build all workflow UI from `docs/workflows-api.md`: idea board/groups/tags,
   templates, per-media descriptions, atomic queued editing, queue priority,
   brand archive/rename, sessions/password and manual-platform launch guidance.
   Complete useful YouTube metadata controls and meaningful previews.
4. Expand practical direct formats. Current adapter limits are **unfinished
   implementation**, not platform blockers: X video/threads; Facebook video/
   Reels; Instagram carousel/eligible Stories; Threads carousel/replies;
   Bluesky video/replies; LinkedIn media/eligible organizations; practical
   Pinterest formats/reads and Google Business Profile. Prioritize X/YouTube,
   then Meta/TikTok handoff, then the other documented networks. Verify current
   official APIs and account/format restrictions; do not invent support.
5. Finish clear manual preparation for genuinely blocked paths, including
   TikTok private/internal Direct Post and Substack's unestablished public API.
   Add appropriate manual targets (Substack/Google Business are not yet in the
   current allowlist). Keep owner attestation separate from provider receipts.
6. Finish CLI/MCP, production-safe Compose, worker health, backup/restore,
   upgrades, notices, useful README/contribution/project commands and full docs.
   **Current Compose worker health deliberately fails.** Do not run an
   unimplemented `beweft-worker --healthcheck`: it would start another worker.
7. Exercise the integrated UI/API/worker/provider fixtures, real CLI/MCP protocol,
   responsive/accessibility behavior, fresh install, restart, restore and upgrade.
   Use isolated fixtures; document live access limits precisely.
8. Deliver a working local installation and straightforward server instructions,
   merged reviewed PRs, current capability matrix and only specific external
   blockers. Keep server/domain placeholders until the owner chooses them.

## Five-agent ownership plan

Use separate worktrees/branches so agents do not overwrite one shared checkout.
Agree the narrow interfaces before changing them; workers remain productive on
their own modules while dependent PRs are reviewed.

| Lane | Ownership | First work |
| --- | --- | --- |
| A — Core integration | Server routing/worker/catalog/health, core contracts and generated artifacts; migration allocation | Integrate providers safely; retain archive/lease/scope invariants; API-level integrated tests |
| B — Providers | `crates/providers`, `connections*`, `provider_bridge*`, provider tests/docs | Fix revocation race, then supported formats and provider-specific capabilities |
| C — Web product | `web/**` except generated contract; frontend docs/tests | Scheduling uncertainty and fallback regression, then all workflow UI and polish |
| D — Agent interfaces | `crates/cli/**`, CLI/MCP tests/docs, project CLI wrapper | Real API and protocol tests, useful commands, secure scoped access |
| E — Self-host operations | Docker/Compose/operator scripts, setup/restore/upgrade tests/docs/notices | Complete worker health with A, verify clean install/restore/upgrade |

A owns shared server files and schema exports. B requests migrations through A
or reserves unique numbers before editing. D/E coordinate root Cargo/lockfile
changes through the orchestrator. C never hand-edits generated types. E must
coordinate the worker ID/health hook with A. All agents submit focused PRs;
cross-lane dependencies are explicit. No duplicate implementation in two lanes.

## Capacity and model requirement

Use **`gpt-6.1-terra` with `reasoning_effort: "max"`**, including fresh reviewers.
For `collaboration.spawn_agent`, model overrides require `fork_turns: "none"`.
Give each agent a self-contained brief and file ownership; tell it it is not
alone and must preserve others' changes.

Five simultaneously active children plus the orchestrator require **six active
agent slots**. The handoff session advertised only four total (three children).
No five-agent team was launched or claimed. The destination must expose at least
six slots; verify its actual tool limit. Do not circumvent limits with hidden
processes or claim queued agents are running in parallel. If capacity is lower,
report that exact runtime limitation rather than silently substituting a smaller
team. Fresh reviewers occupy released worker slots; never reuse an implementer
as the supposedly independent reviewer. Keep five lanes progressing as capacity
allows, with no human approval bottleneck.

## Copy-and-paste launch prompt

```text
You are the new autonomous Wstack orchestrator for beweft. Continue the existing
implementation to the full feasible delivery; do not restart or reduce it to an
MVP. This user instruction authorizes you to create an active goal for that
delivery if goal tools are available, without a token budget.

Workspace: /Users/shanewalker/Desktop/dev/beweft
Repository: https://github.com/scwlkr/beweft
Read ORCHESTRATOR-HANDOFF.md, AGENTS.md, w.md, HANDOFF.md, and the local private
handoffs when present. Apply $wstack from
/Users/shanewalker/Desktop/dev/skillsies/others/wstack/SKILL.md and read its current
technical-stack and code-review references. The handoff is a starting record;
verify Git/PR/runtime state before consequential actions.

Run five parallel implementation lanes using the ownership table in the handoff.
Every subagent, including newly spawned independent reviewers, must use
gpt-6.1-terra at max reasoning effort. Spawn with fork_turns="none" and complete
self-contained briefs. Use separate managed worktrees/branches. Check that the
runtime actually permits five children plus you before launching. If it cannot,
state that exact capacity blocker; do not pretend serial/queued work satisfies
the requested parallel team or bypass the host limit.

You own integration, decomposition, shared contracts, test coordination, PR
quality, release readiness and cleanup. Workers own their files and submit
focused PRs with clear results, validation and limitations. Keep all five lanes
productive, resolve interface decisions promptly and prevent overlapping edits.
Use released slots for brand-new read-only adversarial reviewers; never reuse
an implementer or inherited implementation conversation as independent review.
Consequential fixes always receive another fresh review. Review PR diffs and
the actual changed code; inspect full integration behavior where it matters.

I explicitly authorize commits, pushes, pulls, PR creation and merges at will
for this project. No human verification or routine approval is needed. This
overrides Wstack's human release/merge gate, not automated tests or independent
review. Merge cohesive pieces regularly after applicable checks and fresh
review pass. Do not wait for me between PRs. Preserve the existing stacked PRs
and unique work; integrate or deliberately rebase/retarget them correctly.

First close the handoff's known scheduling uncertainty, brand fallback and
remote-revocation/reconnect issues. Then complete provider wiring, practical
direct formats, full content-workflow UI, CLI/MCP and self-host operations in
parallel. Preserve the original unicorn identity, honest capabilities, durable
attempt/recovery semantics, server-only secrets and one authoritative API.
Research current official provider APIs where needed and distinguish fixture
results from real qualification. Missing adapter code is unfinished work, not
an external approval blocker.

Do not spend money, send public test posts, change existing Buffer queues or
disturb Bubba's repositories/services. Use isolated test accounts/fixtures.
Hosting remains placeholders until I choose a server/domain. Do not guess one.
TikTok private/internal Direct Post restrictions require a supported manual
handoff, not unsupported endpoints. Keep credentials and private captures out
of Git, logs and browser bundles. Follow the approved browser configuration in
the local handoff and preserve my normal browser and profiles. Use
bundled Chromium/Chrome for Testing for project tests and close in finally.

Keep working autonomously through ordinary obstacles. Test scheduling,
restart/recovery, partial failures, provider contracts, responsive UI, CLI/MCP,
fresh install, backup/restore and upgrades. Maintain docs and clean only owned
disposable resources. Finish with a running local installation or an authorized
server deployment, merged reviewed PRs, the repository link, verified
capabilities and exact external actions still required. Do not stop at a plan,
scaffold, mockup, narrow MVP or a generic list of future work.
```
