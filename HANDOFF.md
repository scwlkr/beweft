$wstack

Build beweft: a polished, open-source, self-hosted social media management
web app inspired by Buffer.

This is a full implementation request. Work autonomously from research through
design, implementation, verification, and a usable self-hosted delivery. Do
not stop at a proposal, scaffold, visual mockup, or narrow MVP. Make reasonable
decisions and keep going until the agreed product is built to the extent
platform access allows.

## What I want

I like Buffer's polish and want to learn from its complete experience. I also
like Postiz, but I find its interface archaic.

Use this already-bootstrapped, separate beweft repository for a fresh,
general-purpose app. It must not depend on any specific business's code,
brand, infrastructure, or business data, or on Postiz, Buffer, or another
hosted social management service.

This is a tool for people to run on their own servers and connect to their own
social accounts. It should support multiple brands and channels for one owner.
It does not need subscriptions, customer billing, a commercial hosting service,
or an elaborate agency product.

Keep existing repositories, data, publishing routes, and services intact.
At the start of this request, the bootstrap contained only project notes, the
handoff, a license, and Wstack's starter CLI. Implementation has since begun;
use `w.md` and `ORCHESTRATOR-HANDOFF.md` for current status and remaining work.

## Research and REA

Install [REA](https://github.com/morluto/rea) from its official source and
configure it for Codex. Use an existing compatible Node runtime where
possible; do not install unnecessary native reverse-engineering tools for a
browser investigation.

Use REA and an authorized Buffer account to inspect the entire experience,
including the composer, queue, calendar, ideas, media, analytics, engagement,
connections, settings, and other workflows available to the account. Inspect
the details that make it feel polished: navigation, interactions, previews,
editing, empty states, errors, and platform-specific options.

Use an existing authorized browser profile for Buffer. Do not launch the
installed Google Chrome application as a disposable test browser. For project
browser tests, use bundled Chromium or Chrome for Testing, close it gracefully
in a finally block, and preserve normal browsers and profiles.

Capture a useful, concise feature and behavior inventory. Preserve private
account data and credentials; do not commit raw session captures, tokens, or
private content. Do not publish or change existing Buffer posts or queues
during research. Remove any disposable test material you create.

Use Buffer as the benchmark for usability and polish, and give beweft its own
name and identity. Study Postiz and other open-source projects for useful
integration and product lessons, respecting their licenses.

For social connections, examine [Postiz](https://github.com/gitroomhq/postiz-app)
and two other open-source social managers: [OpenPost](https://github.com/getopenpost/openpost)
and [Mixpost](https://github.com/inovector/mixpost). Read their source and
documentation directly, and use REA to inspect connection workflows when
runtime behavior would add useful insight. Compare how self-hosters configure
provider apps and credentials, how users connect accounts, OAuth callbacks and
permissions, token storage and refresh, reconnecting and disconnecting, and
the guidance and errors shown when setup or provider access fails. Record a
concise, source-backed comparison and use the best ideas to make beweft's own
bring-your-own-connections experience clear and easy. Follow project licenses
if reusing code; no examined service becomes a required publishing backend.

## Preferred stack

Follow Wstack's current instructions and technical preferences. Read the
installed Wstack skill and its technical-stack reference before making choices.

- Backend: Rust, Axum, PostgreSQL, and an OpenAPI contract.
- Web: React, TypeScript, Tailwind, and shadcn/ui.
- Use Zod at the TypeScript boundaries and generate or reuse one API contract
  source where practical.
- Use Cargo, pnpm, and Docker Compose.
- Favor Rust for a new CLI.

Use only the layers and dependencies the product needs. Explain any material
departure from these defaults.

## Product scope

Build the complete practical set of Buffer-inspired workflows discovered
during the investigation, including:

- A polished, responsive desktop and phone web experience, with accessible
  controls and light, dark, and system appearance.
- Multiple brands or workspaces, connected channels, and clear account health.
- Ideas, drafts, a reusable media library, and a multi-channel composer with
  channel-specific text, media, previews, and supported publishing options.
- Per-channel posting schedules, next-available queue slots, exact scheduling,
  reordering, pausing, editing, and a shared calendar.
- An always-on scheduler that continues when my Mac and Codex are closed.
- Per-channel publishing history, provider receipts, failures, and recovery.
  If an outcome is uncertain, reconcile it before retrying so the app does not
  silently create duplicate posts.
- Analytics and a unified inbox where platform APIs and account permissions
  support them. Show unavailable or partial data honestly.
- Connection setup and management, including OAuth, token refresh, revocation,
  capability differences, and helpful setup instructions.
- A documented API and practical CLI/MCP access so agents can inspect work,
  create drafts, schedule posts, and check outcomes through the same
  authoritative actions as the UI.

The interface should feel intentional and cohesive, with the level of detail
that makes Buffer pleasant to use. Use real, functional states and meaningful
feedback—not dashboard filler, fake metrics, or decorative placeholders.

## Social connections

The aim is to bring my own connections and replace Buffer where practical.
Implement direct integrations using each network's supported APIs rather than
requiring Postiz or Buffer as a publishing backend.

Prioritize X and YouTube, then Facebook, Instagram, Threads, and TikTok.
Investigate the other networks and workflows supported by Buffer and implement
the practical ones supported by their documented APIs. Make capabilities
specific to each network and format.

Account for API costs, app registration, OAuth scopes, media requirements,
rate limits, and provider review. In particular, verify TikTok's current rules
for a private self-hosted tool. Where a platform blocks a capability, implement
a clear supported manual handoff where possible and record the exact blocker.
Do not use unsupported private endpoints or claim that a simulated connection
or publication proves a real one.

Use sandbox accounts or isolated fixtures for testing. Do not spend money,
send public test posts, or modify my existing live queues without explicit
instruction. Keep credentials on the server and out of source files, logs,
browser bundles, and the public repository.

## Git and pull requests

Use the bootstrapped repository and verify its GitHub remote early enough to
support continuous delivery. Commit and push regularly in small, cohesive,
reviewable units. Do not leave the whole project for one final commit or PR.

Create focused PRs as meaningful pieces are ready, and merge them regularly
after the applicable checks and fresh independent review pass. Continue with
the next piece without waiting for routine per-PR approval. Follow Wstack's
requirements for implementation agents and independent review, including fresh
review after consequential fixes.

Establish a consistent project standard for commits and PRs in CONTRIBUTING.md
and a GitHub PR template. Use clear, concise titles and explain the problem,
the resulting behavior, and relevant validation. Include material limitations
when they affect the change. Keep the format useful rather than ceremonial.

Keep branches and main integrated as work proceeds. Preserve unrelated work,
never commit secrets or private account data, and report any remote or access
blocker while continuing everything that can be done locally.

## Delivery

Make the app straightforward to self-host: documented setup, persistent
PostgreSQL and media storage, secure authentication and secret handling,
background-worker health, backup and restore, and a clean upgrade path.

The bootstrap includes an MIT license. Maintain a useful README, setup
documentation, and clear contribution information. Make the repository safe
to publish without including private account data or proprietary assets.
Verify and use the existing GitHub remote. Make that repository public if it
is not already public and doing so is appropriate. If the remote is missing or
its destination is unclear, leave the project ready to publish and seek the
destination instead of creating a duplicate repository.

Prepare and deploy it to an appropriate server I control when the target and
access are clear. Do not disturb existing services or guess at a production
domain. If server access or a provider approval blocks a live step, finish
everything that can be completed independently and provide the exact remaining
action.

Follow Wstack's implementation, independent review, and verification
requirements. Exercise real end-to-end behavior, including scheduling,
restart/recovery, partial failures, responsive UI, and provider adapters.
Distinguish fixture results from verified live platform behavior.

Continue through obstacles and complete the full feasible product. Keep useful
project decisions in the repository, clean up disposable files and test
resources, and finish with the running app or deployment instructions, the
repository location, merged PRs, verified capabilities, and any specific
external blockers.
