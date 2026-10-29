# Orchestrator pilot: cycle 1

Historical record: the outcome below describes the bounded pilot. During the
full build on 2026-10-29, PR #11 resolved the brand-list and browser-session
follow-ups and passed a fresh independent review. PR #9 then merged into the
integration branch at `5a42e6e`. See `orchestration-current.md` for current work.

The owner requested one bounded trial of the proposed workflow, with attention
to context use and improvements. This is not a resumption of every product lane.

## Contract for this cycle

- One implementation task: verify and repair brand fallback isolation when the
  active brand disappears and the next brand fails to load. Exercise stale
  responses and composer clearing where relevant.
- A separate read-only workflow critic examines coordination; a brand-new
  read-only code reviewer examines the frozen implementation afterward.
- Every child uses `gpt-6.1-terra`, `max`, and a fresh context. The implementer
  owns a managed worktree, branch, tests and focused PR. The orchestrator owns
  integration and this record, not implementation.
- Start: `a4d724d`, `codex/orchestrator-handoff`. The cycle PR targets that
  integration branch. Existing draft PRs #4–#8 remain intact.
- No live provider actions, normal-browser automation, deployment, or unrelated
  feature work. Existing preview and shared services remain in place.

## Observations during execution

1. **A narrow acceptance case makes delegation concrete.** The brief names the
   user-visible failure, file ownership, exclusions, test isolation, PR base and
   completion report. The worker can act without the parent's full conversation.
2. **PR topology is part of the task contract.** The workflow critic confirmed
   that landing here updates the stack tip/PR #8, not ancestor PR #4. This pilot
   deliberately preserves that stack. Future tasks must name both the landing
   branch and any required propagation to ancestors before implementation.
3. **Capacity limits change what this trial proves.** This runtime permits four
   active agents total. It cannot demonstrate five simultaneous workers. A
   reserved review slot and short-lived workflow critic fit the available limit.
4. **Context needs an explicit reporting boundary.** Workers receive fresh,
   self-contained briefs and return a compact checkpoint: SHA, PR, files,
   checks, limitations and owned resources. The parent reads decisions and
   failures, rather than replaying command logs or implementation conversations.
   No token-saving percentage is claimed; no token accounting is available.
5. **Negative controls helped, but did not replace review.** The first regression
   passed and failed when either cleanup or the late-response guard was removed.
   The fresh reviewer nevertheless found a different dependency failure:
   successful brand discovery was gated by unrelated provider/health requests.
   A failed or hanging optional request could prevent the required cleanup.
   The cycle returned to the implementer for a narrow fix and new coverage.
6. **Scope the failure boundary before writing a test.** For asynchronous UI
   work, the task brief should identify which response controls identity/access
   and which responses are optional metadata. A small list of failure orderings
   would have exposed this missing case earlier. This is a proposed improvement,
   not a measured speedup.
7. **Keep review resources until review is finished.** The first worker cleaned
   temporary dependency links/build outputs before handing off. That is safe,
   but a reviewer who runs tests has to recreate them. Prefer transferring clear
   ownership of needed temporary resources to the orchestrator, then cleaning
   once after review/integration. Never share mutable outputs across concurrent
   builds without coordination.
8. **The second brief was still too loose about recovery.** The next reviewer
   reproduced a continuing metadata outage, a suppressed brand-removal response
   during overlapping refreshes, and delayed handling of a 401. The tests had
   simulated only an initial metadata failure. The acceptance condition now
   includes usable replacement content during a persistent outage, prompt
   authentication handling, and overlapping successful brand snapshots.
9. **A short design exchange improved the next iteration.** Before another code
   change, the implementer proposed independent request lanes. A separate critic
   identified two distinctions: session invalidation versus brand selection,
   and completed successful responses versus newly started requests. The parent
   chose that approach with those refinements. This avoided asking the parent
   to inspect or redesign the full hook in its own context.
10. **Worker-owned Git delivery reduced integration bookkeeping.** The worker
    committed, pushed and maintained its PR from an isolated worktree. Parent
    documentation edits stayed in the main checkout. The parent did not have to
    split interleaved source edits or manipulate a shared staging area.
11. **Retained builds need a source identity.** Negative controls temporarily
    alter application code and bundles. Before the third frozen revision, the
    worker restored mutations and confirmed a final-source rebuild was identical
    to the bundle used by the passing suite. Reuse must not accidentally hand a
    reviewer a deliberately broken experimental bundle.
12. **Completion messages can be simpler.** A frozen milestone followed by an
    extra parent request to finish adds coordination noise. Require one final
    checkpoint report that ends the worker turn; the orchestrator should await
    the completion event instead of requesting acknowledgement.
13. **Shared dependency links were less reusable than the handoff implied.**
    pnpm 11 attempted dependency verification/reinstallation in the review
    worktree and refused a noninteractive removal. The reviewer used
    `pnpm_config_verify_deps_before_run=warn`; forcing `CI=true` could have
    removed the shared target. Prefer each worktree's own installation backed
    by the package store. If links are reused, hand off the exact safe command
    and environment, not just paths. Directory ignore patterns also did not
    hide these symlinks from Git, so explicit staging and unlinking mattered.
14. **Review scope needs an explicit decision.** The third reviewer reproduced
    direct-composer 401 handling and a delayed logout affecting a new session.
    Comparison with the base showed both were pre-existing outside the changed
    flow. They are recorded as follow-ups, not silently fixed or used to expand
    this trial. The documentation's overly broad authentication claim was
    narrowed. The same review found an actual remaining brand-list race, which
    still blocks this PR.
15. **I should have defined the pilot's stop condition earlier.** One task grew
    into three independent review rounds and two rework rounds. For a bounded
    experiment, define a review/rework limit or an explicit stopping gate before
    starting. This does not impose a stop limit on the separately authorized
    full-product build. Here the trial ends at the third review gate, with the
    unresolved work preserved rather than merged.

## Outcome

The bounded trial is complete; the implementation is **not approved for merge**.

- [PR #9](https://github.com/scwlkr/beweft/pull/9), branch
  `codex/cycle1-brand-recovery`, remains draft against `codex/orchestrator-handoff`.
- Last reviewed code: `1b519c3e6b44474c67deed29d68ce0c74b24db2b`. Final head
  `a6b3d9f9ace1caa53dfce2cfe91cbd6a868881e7` changes only documentation after review.
- Build and all 28 isolated Chromium scenarios passed. The third reviewer also
  ran the focused case and full suite, and checked the retained build against
  clean source. No live provider behavior was qualified.
- Remaining repro: an older successful brand snapshot selects B after A is
  archived. A newer successful snapshot excluding B is discarded because that
  automatic fallback changed the selection epoch. Hold B's follow-up refresh:
  archived B stays selected. Add this regression and fix the distinction between
  automatic fallback and manual selection, then get a new independent review.
- Two pre-existing authentication follow-ups are documented in PR #9 and its
  frontend documentation. They were not fixed in this cycle.
- Main and the handoff branch receive no pilot application code. The existing
  preview remains unchanged. Scheduling uncertainty, provider revocation,
  provider wiring and operational delivery remain outside this trial.

The orchestrator consumed compact task/review reports and inspected the hook
diff only when resolving a scope question. This protected it from full worker
logs; it does not demonstrate a measured token saving or five-agent throughput.
Closeout: the final branch head was verified on GitHub. The three recorded
dependency/build symlinks were unlinked without touching their shared targets;
the worktree's generated web bundle and build metadata were removed. Codex
confirmed the managed worktree archived. No isolated browser-test databases or
task-owned processes remain. The original preview API/worker remain running.
The findings and updated launch guidance are committed to the handoff branch;
the application changes remain only in draft PR #9.
