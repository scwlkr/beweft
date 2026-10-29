# Connect your own social accounts

beweft talks to each network directly. You supply your own developer app and authorize your own destinations. Neither Buffer nor another social manager is a publishing backend. API access, provider review and costs remain your responsibility; installing an adapter does not grant them.

The direct adapters are fixture-tested, not live-qualified for your accounts. Read [capability research](research/provider-capabilities.md) for dated API rules and [connection design](research/connections.md) for the source review.

The provider connection handlers are not yet wired into the production router or UI. The workflows below describe the adapter design and are not a claim that live connections can already be made through this build.

## Prepare the server

1. Set `BEWEFT_PUBLIC_URL` to your actual HTTPS origin. Do not add a path or invent a domain; it must route to this server.
2. Set `BEWEFT_ENCRYPTION_KEY` to a persistent random 32-byte key encoded as 64 hexadecimal characters. Keep it outside Git and back it up separately from the database. Losing it makes saved provider credentials unreadable.
3. For Mastodon or a custom Bluesky PDS, set `BEWEFT_ALLOWED_PROVIDER_ORIGINS` to comma-separated HTTPS origins approved by the operator. The default is `https://bsky.social`. Origins must use public DNS names and port 443. beweft resolves and pins public addresses for the operation and does not follow HTTP redirects. Private/local addresses are rejected.
4. Ensure your reverse proxy allows the configured upload size and long-running uploads. The media library persists on the server; worker processes need the same media volume.
5. Confirm worker health before filling queues. App sign-in, OAuth consent, successful publishing and receipt verification are separate checks.

## Set up a developer app

Open connection settings, choose the provider, then enter its client ID and secret. Saved secrets are never returned to the browser. Leaving an existing secret field omitted preserves it. Supply secrets through a password manager; never paste them into issues or logs.

Register exactly this callback pattern, replacing the origin and provider slug:

```text
https://your-real-host.example/api/v1/connections/x/callback
```

| Provider slug | Developer setup and destination |
| --- | --- |
| `x` | X developer app with OAuth 2 enabled, confidential web-app client, read/write and media permissions. Approve pay-per-use costs in the provider console first. |
| `youtube` | Google Cloud project with YouTube Data API enabled, OAuth web client and consent configuration. Authorize the Google identity owning the intended YouTube channel. Check the actual project quota and verification state. |
| `facebook` | Meta app with Facebook Login and Page permissions. Consent as a person who can create content on the Page, then choose that Page. Personal profiles are not Page destinations. |
| `instagram` | Meta Instagram Login app; Business or Creator account. This adapter uses the Instagram Login route, not a Facebook-linked personal account. |
| `threads` | Threads use case in a Meta app, Threads scopes and Threads identity. A Facebook token cannot substitute. |
| `mastodon` | Register an OAuth application on the operator-approved instance with the exact callback. Supply its client credentials and instance origin. |
| `linkedin` | LinkedIn app with OpenID Connect and Share on LinkedIn. This slice connects member profiles; organization access is a separate capability. |
| `pinterest` | Pinterest business developer account and reviewed trial/standard app access. Configure the exact callback and select an authorized board ID when composing. |
| `bluesky` | Save the configured PDS origin; client ID and client secret may be empty. Connect with handle and a dedicated app password. The password is exchanged for session tokens and is not retained. |

Exact scopes requested by the installed version are listed in `crates/providers/src/oauth.rs`. Optional analytics/comment scopes may require additional provider approval. If a developer app rejects a scope, correct its products/approval before reconnecting; do not assume a publishing-only grant enables the whole inbox.

## Connect and check the identity

Choose a brand, connect its channel, complete provider consent, and choose the returned destination. The flow expires after ten minutes and must finish in the browser session that started it. Starting another flow replaces that browser's temporary correlation cookie; finish one at a time. The UI reports authorization/identity confirmation without claiming publishing has been tested.

On reconnect, authorize the same external destination. A different Page/channel must be connected separately. Reconnect retains schedules and history; an intentionally paused channel stays paused. Disconnect cancels queued deliveries, removes local credentials and pauses the selected channel. The connection UI must warn before disconnect that other connected channels for the same provider, including unrelated accounts, can require reconnection. Their saved credentials, pause settings and queues are retained. The provider handlers and this warning still need production route and UI wiring.

OAuth authorizations started before a disconnect must be started again after access removal. Disconnect never sends a provider revoke request; for every direct provider, the owner inspects existing channels, removes the affected access through the provider's access settings (such as connected apps or app passwords) and explicitly confirms completion before connecting again. This confirmation is an owner attestation, not a provider receipt. Grant sharing is not reliably known, so a stored grant or an unhealthy/unknown direct grant even without saved credentials conservatively marks other connected channels for that provider as needing reconnection, including unrelated accounts. Their saved credentials, pause settings and queues are retained. An active delivery or reply on an affected channel blocks disconnect; an active sync must finish first. While a provider operation is in progress, wait for it to finish and retry to check the current status before removing access. An unresolved removal blocks new authorization and completion across that provider. Repeating a completed disconnect with no saved grant does not restart removal or affect channels reconnected since then; a still-pending removal remains pending. When there is no known or possible direct grant, inspect existing channels before removing only orphaned provider access. Google says [revocation removes the project's granted scopes and invalidates issued tokens for all clients in the project](https://developers.google.com/identity/protocols/oauth2/web-server#tokenrevoke) and may take time to have full effect; neither fixtures nor owner confirmation establish live propagation timing.

Callback code exchange and identity lookup hold the same provider gate as disconnect, and a flow from an older generation cannot begin an exchange after the removal boundary. A callback received while the gate is busy consumes that authorization; wait for the current operation or removal confirmation. An interrupted or timed-out exchange can still have created access remotely even when beweft stores no candidate. First inspect your connected channels for the destination: a completion response may have been lost. If it did not connect, check the provider's connected-app settings and remove only orphaned access before starting authorization again; shared provider access may serve other channels. Canceling a local request does not prove the provider canceled its work. Bluesky app-password session creation also holds the provider gate and is blocked while removal is unresolved; an interrupted session request may still have created a remote session.

## Current formats and observations

The provider catalog is authoritative for this installed slice. X publishes text/images; YouTube uploads a video with explicit privacy and audience; Facebook publishes text/a single image; Instagram publishes one JPEG/Reel; Threads publishes text/single image/video; Mastodon publishes text/media; Bluesky publishes text/images; LinkedIn publishes member text; Pinterest publishes an image Pin. Unsupported formats return a clear error instead of dropping attachments. These are implementation limits to extend, not claims that the networks block all other formats.

Remote-fetch formats (Meta and Pinterest) need your public HTTPS origin. beweft creates expiring capability URLs for only the media in that delivery; the authenticated media library remains private. Do not log or share capability URLs. YouTube uploads stream in 8 MiB chunks, saving the session and confirmed offset between requests. These checkpoints are encrypted and never returned by the public API.

Use channel sync to fetch a bounded recent page of available comments/mentions and observations for up to ten local published posts. X sync requires explicit metered-read acknowledgement. This is partial coverage, not a historical import or universal DM inbox. Missing metrics stay missing; the local workflow summary is not social engagement data. Replies are supported only where the catalog says so and have durable request IDs; reusing a request ID reads its result without sending twice.

Concurrent syncs share a per-server budget derived from its database pool, leaving one pool connection outside that budget. When the budget is full, a sync is rejected promptly and can be retried. A pool with only one connection cannot run syncs. This limits sync admission; unrelated operations can also use the remaining connection.

## Resolve failed or uncertain delivery

A definite provider rejection can be corrected and retried through the application. A lost response or worker interruption is **uncertain** and never automatically becomes another post. For YouTube, provider reconciliation probes the saved upload session and can resume confirmed partial progress. For Bluesky, it queries the preassigned record key and compares the saved content. Other uncertain outcomes need inspection on the network and explicit owner reconciliation. A missing receipt is not proof that publication failed.

TikTok is manual-only for this private self-hosted use case: its Direct Post intended-use rules exclude internal/private upload utilities. Download the media, copy the caption, finish on TikTok, then record completion and its URL. Manual confirmation is an owner attestation, not a verified API receipt.

Provider formats, permissions and pricing change. Do not run public test posts or paid calls merely to check setup. Use approved isolated test accounts/fixtures, and separately authorize any live qualification.

### Grant and audience safeguards

Each channel's `capabilities` describes the implemented adapter separately from `capabilities.permissions`. The latter reports provider-returned scopes and `granted`, `missing`, or `unknown` for each action. Missing required scopes block that action before a request. Unknown means the provider did not report enough scope information; it is not evidence of permission or successful publishing. Even a granted scope does not prove account eligibility or provider app review. Refresh preserves scope knowledge when the provider omits it.

A stored grant is bound to its original client ID and instance origin. Changing either requires authorization again. Tokens are never moved to the new origin. Federated destination IDs include their instance origin, so matching numeric IDs on two servers cannot reconnect one another. In-flight OAuth authorization also records its original app; configuration changes invalidate that flow. Existing pre-upgrade flows must restart.

Mastodon inbox items retain original visibility and the source author/mentioned participants. A reply to a direct **or followers-only** source is sent **directly to those participants**: using followers-only on the reply could disclose it to a different follower set. Public and unlisted sources retain their visibility. Replies cannot add `@` mentions; the server prefixes the preserved recipients and supplies `allowed_mentions`. Missing audience metadata requires a fresh sync or a reply on the network. This follows Mastodon's [status controller](https://github.com/mastodon/mastodon/blob/main/app/controllers/api/v1/statuses_controller.rb) and [posting service](https://github.com/mastodon/mastodon/blob/main/app/services/post_status_service.rb), including explicit visibility and unexpected-mention rejection.

Reply claims and channel disconnect share the channel transaction lock. A claimed active reply prevents disconnect; a disconnected grant cannot claim a new reply. A reply past its stored delivery deadline becomes uncertain, never automatically resent. New claims record the actual database clock and a matching 90-second HTTP deadline after any lock wait or refresh. Reusing its request UUID returns its existing result.

### Media inspection and recovery

Install `ffprobe` (the Compose image includes FFmpeg). Before uploading media, the adapter inspects its bytes using a forced local media container, disabled MOV external references, a file/pipe protocol allowlist, a 15-second deadline, bounded output and no inherited credentials. Missing inspector, malformed containers, unsupported MIME/codec, empty/oversized media, incompatible attachment combinations, missing dimensions and invalid durations fail before a publishing request. File-backed video is streamed after inspection; it is not loaded into RAM in full.

Current conservative adapter limits: X images 5 MiB; Bluesky images 2 MB; Facebook image 10 MiB; Pinterest image 20 MiB; Instagram/Threads images 8 MiB; Mastodon images 16 MiB and video 99 MiB. Other video is bounded at 256 MiB as well as the server upload setting. Images have a 40-megapixel/16384-pixel ceiling (Bluesky 4000×4000); Instagram photos require width 320–1440 and aspect ratio 4:5–1.91:1. Instagram video requires 3–900 seconds; Threads video at most 300 seconds; both require MP4 H.264/HEVC, optional AAC and width at most 1920. YouTube accepts inspected video up to 12 hours, subject to the actual channel's upload eligibility. These are this adapter's accepted envelope, not a claim that every provider/account has identical limits. Mastodon instances may impose stricter limits. Automatic transcoding is not implemented. GIF delivery is currently accepted only for Mastodon; animated media cannot be mixed with other attachments.

Sources: [Bluesky image lexicon](https://github.com/bluesky-social/atproto/blob/main/lexicons/app/bsky/embed/images.json), [X media guidance](https://help.x.com/en/using-x/posting-gifs-and-pictures), [Meta's Instagram Reels sample requirements](https://github.com/fbsamples/reels_publishing_apis/tree/main/insta_reels_publishing_api_sample), [Mastodon attachment limits](https://docs.joinmastodon.org/user/posting/#attachments). Provider validation still has the final say about account-specific eligibility.

A YouTube upload status probe returning 404 marks the saved session expired; it does **not** prove the video was never published. Inspect YouTube Studio and record the authoritative owner outcome. If recorded as not published, explicitly reset the expired session, then retry. Reset is rejected while uncertain, for a live session, or without a later not-published attestation. It never happens automatically. This follows [Google's resumable upload protocol](https://developers.google.com/youtube/data/guides/using_resumable_upload_protocol). Checkpoint updates and reconciliation transitions hold the publication lock; worker checkpoint writes additionally require the current live attempt UUID, protecting newer attempts from stale workers.

### Isolated regression checks

`cargo test -p beweft-providers --tests` runs loopback HTTP fixtures and generated media checks (requires FFmpeg). `python3 crates/server/tests/provider_integration.py` creates and removes a unique database in the existing development PostgreSQL container, then exercises OAuth replay/session/config binding, blocked callback exchanges and stale generations, all-provider owner-managed removal fences and stale reconnects, bounded concurrent syncs and token rotation, origin-aware reconnect, reply/disconnect races, stale-checkpoint fencing, concurrent reconciliation, and expired-session reset. It removes its generated media and test database in cleanup. These checks establish fixture behavior, not live provider qualification.

The server's `provider-fixtures` Cargo feature exists only for these tests; it enables an explicitly supplied loopback-only `BEWEFT_PROVIDER_FIXTURE_ORIGIN`. Never enable this feature in a deployment build. The standard build ignores that variable entirely.

Before each provider stage the worker locks its channel, publication and grant, verifies the original attempt UUID and live lease, and reads the immutable snapshot on the same database connection. It holds these locks through the request and checkpoint update. Every adapter HTTP request, including streaming uploads, receives an absolute deadline bounded by the remaining lease (with a one-second margin). A deadline or lost response remains uncertain. Reconnect waits for the active stage, so an old grant's 401 cannot invalidate a newly connected grant. Reads and writes performed inside a transaction reuse its connection; they do not depend on spare pool capacity.

Inbox reply history is durable and available read-only across browsers. New reply UUIDs cannot bypass an unresolved sending/uncertain attempt. If an uncertain reply cannot be reconciled through a provider API, the owner must inspect the network and record published or not-published with a note. This is labeled owner attestation, never provider confirmation, and never resends automatically.

New replies also carry the last acknowledged reply ID as an optimistic history watermark. If another browser completes a reply while an earlier request is delayed in transit, that delayed request is rejected with `reply_history_changed`, even though the newer reply is already terminal. A same-UUID retry still retrieves its original result. History order is durable claim order rather than transaction-start timestamps. Migration `0006_provider_reply_history.sql` adds this ordering; standard upgrades apply it automatically.

Publishing preparation uses separate committed phases. First, the original attempt and lease are checked under locks and any successful refresh-token rotation is committed immediately. Media capability URLs are then committed before the provider can fetch them through another HTTP/database connection. The worker reacquires the locks and rechecks the original attempt, lease and current grant before publishing; those locks remain through the request and checkpoint save. Reconciliation and reply claims likewise commit refresh separately from later fallible work. A missing checkpoint, inconclusive probe, media error or publishing timeout cannot roll back a completed token rotation.

Migration `0007_reply_delivery_deadline.sql` gives replies a durable delivery deadline. This prevents a transaction that waited for a lock or token refresh from making a newly sent reply appear expired. The callback-fetch and single-use refresh-token regressions exercise these transaction boundaries against real HTTP and PostgreSQL fixtures.
