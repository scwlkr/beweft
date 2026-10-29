# Buffer behavior inventory

Observed 2026-10-29 in an authorized existing browser session. This record keeps
product behavior only; account identifiers, private content, raw captures and
credentials are deliberately excluded. Research did not create, save, publish,
retry, reorder, or delete account content.

## Observation boundary

The existing Chrome profile was inspected through native browser accessibility
tools, with REA passive CDP observation added after the owner explicitly approved
enabling remote debugging permanently. REA's exact-origin filter limited the
observation to Buffer; text, script source, storage keys/fingerprints, console text
and response-body capture remained off. Account plan limits prevent inspection
of paid-only workflows. No private endpoints were invoked directly.

## Workflows and details worth carrying forward

| Surface | Observed behavior | Beweft direction |
| --- | --- | --- |
| Navigation | Persistent primary sections, channel shortcuts, contextual subnavigation, organization menu | Brand switcher, persistent sections and channel filters; accessible phone navigation |
| Home | Recent drafts, upcoming publications, unanswered comments, time-bounded insights; clear links into actual work | Start with actionable work and setup guidance; no invented scores or filler |
| Publish | Queue/drafts/sent tabs, list/calendar switch, channel/tag filters, timezone display; failures grouped separately | Shared records across list/calendar, visible failures and uncertain outcomes |
| Composer | Modal with fullscreen option; shared content first, customize for each network second; selectable channels; preview toggle | Shared content plus explicit channel overrides; full-screen phone composer |
| Editing | Tags, file upload/drop, emoji, templates, optional AI assistance, character budget, network preview | Functional core controls; validation adjacent to each channel; preserve edits on errors |
| X | Character budget, thread affordance, link behavior, AI disclosure | Only options supported by the direct adapter; explain unavailable formats |
| YouTube | Short format, title, category, visibility, license, notify subscribers, embedding, made-for-kids, AI disclosure | Explicit video metadata and eligibility validation before scheduling |
| TikTok | Text budget, media requirement, publishing method, AI disclosure | Supported manual preparation when private-tool API eligibility blocks direct posting |
| Timing | Next Available, Prioritize, Now, Set Date and Time; explanatory descriptions; save draft independently | Distinguish reserved queue slots from exact schedules; show channel timezone |
| Calendar | Week/month selector, previous/next/today, channel/status/tag filters, undated drafts, time-grid entry points | Calendar and accessible agenda view using real publication records |
| Ideas | Board/gallery, named status columns, title/body/media/tags, drag reorder, create post from idea | Capture and organize ideas; convert without losing original context |
| Media | File picker/drop in composers and idea editor; Canva, Dropbox, Google Drive/Photos, OneDrive and Unsplash integration menu | Dedicated reusable server media library, metadata/alt text and safe uploads |
| Community | Post/comment views, comments/mentions tabs, channel/status filters, sort, resolve; thread, source link, reply count, character budget, saved replies | Capability-aware unified inbox with sync state, source links and explicit reply action |
| Insights | Date windows/comparisons, per-channel results, top posts, metric definitions, sync status, export; no-data messages | Display available provider metrics with coverage and freshness; unavailable is not zero |
| Channel settings | IANA timezone, weekly slots, posting goal, per-day enable, add/remove slots | Transactional schedules with DST policy and upcoming-slot preview |
| Queue controls | Pause, shuffle, clean failures, empty queue, delete drafts | Pause/reorder and explicit recovery; avoid bulk destructive actions without clear scope |
| Connections | Provider chooser, account type, connected/locked states, per-channel settings and menus | Separate server app setup from account authorization, permission health and reconnect |
| Settings | Profile, preferences, notifications, organization, roles, tags, saved replies, API and integrations; appearance, timezone, time format, week start, landing page and default posting action | Owner-focused settings, appearance, credentials guidance, scoped API access |

The observed connection chooser lists Instagram, Threads, LinkedIn, Facebook,
Bluesky, Substack, YouTube, TikTok, Mastodon, Pinterest, Google Business, X and
Buffer's own Start Page. A chooser entry does not establish public API availability.
Provider implementation decisions require official API evidence.

## Product boundaries

Billing, plan locks, agency roles, commercial growth prompts, proprietary AI,
Buffer-hosted pages and Buffer integrations are not dependencies or requirements
for beweft. The useful lessons are interaction clarity, nearby previews,
contextual feedback, consistent navigation and precise platform options.

Further paid-only workflows and media-dependent composer options have not been
verified. No intentionally induced live error or successful publication was tested.
