# Beweft Roadmap

This roadmap tracks progress toward Buffer-level functionality. It is continuously updated as work progresses.

## Completed
- [x] **Home dashboard** (Nov 2 2026) - Buffer-style home with upcoming posts, latest drafts, quick actions, and week overview
- [x] **Navigation refresh** - Added Home as default route with cleaner organization
- [x] **Improved scheduling UX** (Nov 2 2026) - Card-style options for draft/queue/schedule like Buffer
- [x] **Queue labels polish** (Nov 2 2026) - Queue/Sent terminology
- [x] **Content Planner copy** (Nov 2 2026) - Ideas renamed to Content planner

## In Progress
- [ ] Continue improving UX to match Buffer's polish

## Completed (Recent)
- [x] **AI assistance** (Nov 3 2026) - Post suggestions and caption improvements in composer
- [x] **Keyboard shortcuts** (Nov 3 2026) - Power-user navigation/composer actions
- [x] **Calendar enhancements** (Nov 3 2026) - Drag-and-drop rescheduling, week/month views, posting slots visualization
- [x] **Queue improvements** (Nov 3 2026) - Status/channel/sort filters, clearer bulk actions, select-all, improved empty states
- [x] **Queue status filtering** (Nov 2 2026) - Filter by queued/sending/published/failed/uncertain/needs handoff
- [x] **Queue bulk actions UI** (Nov 2 2026) - Clearer selection and actions
- [x] **Templates copy polish** (Nov 2 2026) - Templates library naming
- [x] **Calendar quick actions** (Nov 2 2026) - New post button in toolbar
- [x] **Calendar posting slots visualization** (Nov 2 2026) - Show channel posting schedule times on calendar
- [x] **Analytics/Insights copy polish** (Nov 2 2026) - Clearer labels and messaging
- [x] **Queue needs-attention badges** (Nov 3 2026) - Highlight failed/needs attention counts
- [x] **Home ideas count** (Nov 3 2026) - Show real idea count on dashboard
- [x] **Home quick actions** (Nov 3 2026) - Added View queue quick action
- [x] **Calendar day dialog** (Nov 3 2026) - Richer day view with quick scheduling
- [x] **Character limits** (Nov 3 2026) - Per-platform character counters in composer and preview
- [x] **Dark mode polish** (Nov 3 2026) - Improved contrast and colors
- [x] **Analytics filters, top posts & CSV export** (Nov 3 2026) - Server-side channel/date/limit filtering on `/analytics` (authorized channel scope, scheduled-time window), web channel picker + 7/30/90-day/all range, top-posts ranking by summed metric values, and CSV export with per-metric columns. Verified: `cargo check -p beweft-server` ✓, web `tsc -b` ✓, `vite build` ✓ (652.98 kB JS, 48.78 kB CSS), regenerated TS contract shows `channel_id/from/to/limit` on `GET /api/v1/analytics`.
- [x] **Queue CSV import fix** (Nov 3 2026) - Replaced silent-failure import: dialog with explicit brand-scoped channel picker (no `channels[0]` guess), downloadable template, header detection (legacy headerless files still work), 50-row cap with skipped-empty-row count, and visible success/partial-failure report with per-row error messages. Posts import as drafts for the selected channel only. Verified: `cargo check -p beweft-server` ✓, web `tsc -b` ✓, `vite build` ✓ (655.16 kB JS).

## High Priority (Remaining)
- [x] **Content Planner ideas flow** (Nov 2 2026) - Clearer "Turn into post" action
- [x] **Community/Inbox polish** (Nov 2 2026) - Improved messaging and visibility
- [x] **Analytics/Insights enhancements** (Nov 2 2026) - Better empty states and messaging
- [x] **Post preview improvements** (Nov 2 2026) - Clearer preview messaging
- [x] **Mobile responsiveness** (Nov 2 2026) - Better mobile layouts for composer/toolbar
- [x] **Media library UX** (Nov 2 2026) - Search and filter by type/name/description

## Nice to Have
- [x] **AI assistance** (Nov 3 2026) - Post suggestions, caption improvements
- [x] **Bulk scheduling** (Nov 2 2026) - CSV import UI placeholder (now functional)
- [x] **First comment scheduling** (where supported) (Nov 3 2026) - FB/IG/Threads first comment with post-publication follow-up
- [x] **Link-in-bio** style features (Nov 3 2026) - Start Page with customizable links, public sharing
- [x] **Dark mode polish** (Nov 3 2026)
- [x] **Keyboard shortcuts** (Nov 3 2026)

## Notes
- Focus on scheduling, publishing, queue management, analytics, account management, and responsive design (per objectives)
- Preserve existing functionality and conventions
- Test actual workflows
- Match Buffer's core functionality where practical, aim for cleaner cohesive UI
