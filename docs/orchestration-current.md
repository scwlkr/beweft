# Active delivery record

The full build remains active. The practical local release target is
2026-10-29 at 16:30 America/Chicago; that checkpoint does not erase unfinished
product scope or establish live-provider qualification.

## Landed application

The original PR stack #4–#8 is merged into `main` with merge commits and its
ancestry intact. Main merge `a60b531` has exactly the application tree reviewed
at `3b4445b`. Reviewed continuation PRs through #23 include owner/brand/session
recovery, scoped CLI/MCP, worker health, provider removal fences, media
metadata, idea organization, durable scheduling receipts, production provider
wiring, X video, and connection/manual/reply recovery UI. Bluesky video and
its explicit quota-rejection retry correction also landed through PR #26.
Live provider access remains unverified.

A fresh independent reviewer exercised the built image through first-owner
setup, invalid setup secret/timezone, empty initial data, manual channel,
posting rule, media/description/bytes, draft, queue and exact schedules, worker
handoff, owner-attested history, restart, health and sign-in. External browser
requests were blocked. This was real local UI/API/worker use, not live posting.

The real image/schema 7→14 upgrade preserved owner access, media, draft and
queue; restart/worker health, subsequent upload and matching old-image/data/key
rollback with a held worker passed. PostgreSQL major-version migration and a
physical host reboot have not been qualified.

The retained local installation is healthy on loopback port 3000, now upgraded
backup-first to image source `3b4445b` and schema 14. First-owner setup remains
uninitialized so the owner chooses their identity and password. It has no demo
content or provider credentials. Private installation/configuration and rollback
material remain ignored; never print or commit their secret values. The earlier
dev database/media remain preserved separately.

## Current work

| Milestone | State |
| --- | --- |
| Meta formats #24 / correction #28 | Merged at `6ae032d` after fresh review of frozen `394ea493`. Accepted Facebook Reel processing remains sending/uncertain and cannot become an ordinary cancelable queued item. The cumulative merge also landed #24 and #27. |
| Content/owner UI #25 | Merged at `acf0892` after fresh review of frozen `800f225`. Audit-linked replacement recovery, templates, queued edits/priority, brand archive/restore, owner sessions/password, Meta and LinkedIn recovery are included. Seventeen focused browser scenarios and real API workflows passed. |
| Bluesky video #26 | Merged at `1320d7e` after fresh review of explicit quota-rejection retry. Job/blob checkpoints, stable record identity, PDS compatibility guard and bounded notification reads are included. Replies remain unfinished and disabled. |
| LinkedIn media #27 | Member image/video stages and exact-ID owner recovery passed practical independent review at `899c370` and landed through cumulative #28. Organization opt-in/discovery remains unfinished. |

Each consequential revision still receives a brand-new read-only reviewer.
All implementation/review agents use `gpt-6.1-terra` at `max`, with at most three
active children plus the orchestrator. New milestones rotate implementers;
unfinished fixes keep their owner. Review blocks broken core use, likely data
loss, credential exposure, accidental public actions and other serious practical
defects. Uncommon recoverable hardening findings are recorded rather than used
for repeated exhaustive loops.

## Fixed contracts

Scheduling saves an immutable owner/brand/post-scoped UUID, draft revision,
channels, exact timestamp and complete effective media-version map before the
write. Missing receipts and in-progress/error responses remain unknown. Replay
returns original publication identities; retirement never cancels accepted work.
Exact times retain UTC microseconds. Media description writes use CAS versions;
new publication snapshots freeze their descriptions.

Provider upload/container IDs and public-write boundaries survive worker claims.
Unknown public outcomes must not cause blind automatic reposting. For an accepted
remote operation that may still finish, a stopped local sender and an absent post
are insufficient evidence to retry: the remote operation must have definitively
ended without publication. X recovery binds
owner inspection and sender-stopped attestation to observed attempt/checkpoint
IDs, then requires explicit reset and retry. Reply recovery uses observed
revision and active-executor guards. An owner attestation is not provider proof;
a lost database session alone does not prove an outbound sender stopped.

Provider fixture tests require both matching build/startup guards and a verified
loopback-only OS sandbox. Keep mutable default and feature targets separate.
An earlier synthetic OAuth run exposed why those controls are necessary; no
real credentials or public content were involved. No paid calls, public test
posts, Buffer changes or unrelated services are authorized.

## Remaining full scope and cleanup

The reviewed source milestones through #28 have landed; the combined local
image is being packaged and qualified. Additional local implementation still includes X threads, standard
Facebook Page video, Instagram Stories eligibility, Threads/Bluesky replies,
LinkedIn organization opt-in, expanded Pinterest formats/reads, Google Business
Profile direct support and safe Google grant-family handling. Bluesky entryway
to PDS onboarding/reconnect migration must be implemented without silently
moving existing grants or queues. Missing code is not an external approval block.

Hosting stays placeholders until a server/domain is selected. Operator-specific
app registration, scopes, account eligibility, costs/consent and live provider
qualification remain separate from fixture verification. The local host must
remain running; an always-on server is still an owner choice.

Completed review worktrees are retired through Codex; never bypass its primary,
shared or active-checkout guard. Fully merged inactive branches are removed only
after checking ancestry, open PR bases and worktree ownership. Unique research
and manual-guidance history remain protected. Disposable databases, processes,
media and tests are cleaned after use; active caches and recovery material have
explicit ownership until their work finishes.
