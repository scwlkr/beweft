# Provider capabilities and access

Research checked 2026-10-29. This is an implementation decision record, **not** a list of verified beweft integrations. A provider's documented API capability, implemented adapter, passing fixture, approved developer app and verified live account are separate facts. No paid calls, account connections or publications were made during this research.

## Priority networks

| Network | Supported API path and requirements | Product treatment |
| --- | --- | --- |
| X | User authorization; OAuth 2 authorization code + PKCE supports scoped v2 access. Request `tweet.read`, `tweet.write`, `users.read`, and `offline.access` for publishing/identity/refresh, adding media permission when required by the selected upload endpoint. `POST /2/tweets` creates a post. [X1, X2] | Text and supported media need separate validation. A paid permission failure is not a reconnect problem. Show API cost warning before enabling sync or delivery. |
| YouTube | Google OAuth web client, Data API enabled, `youtube.upload` for uploads; identity/reads and analytics require additional scopes. Resumable uploads should retain the session URI and returned video ID. [Y1–Y4] | Require title, video and explicit privacy; support audience/synthetic-media disclosures and processing state. Never assume upload completion means public availability. Shorts are videos subject to YouTube classification, not a separate guaranteed upload endpoint. |
| Facebook | A Meta app authorizes the user and exposes managed Page tokens and tasks; select a Page, not the user's personal profile. Meta's official collection demonstrates Page Reels create/upload/status/publish stages. [M1] | Page publishing and its individual media formats require app permissions and Page tasks. Preserve the upload/video ID for reconciliation. Personal-profile and Group handoff must not masquerade as Page API publishing. |
| Instagram | Professional Business/Creator accounts; Facebook Login requires a linked Page. The Instagram Login route uses `instagram_business_basic` / `instagram_business_content_publish`; Facebook Login uses `instagram_basic` / `instagram_content_publish` plus Page permissions. [M2] | Model login routes separately. Images, carousel, Reels and Stories have different requirements; Stories eligibility is narrower. Container creation, processing and publication are distinct stages. Personal accounts use manual handoff. |
| Threads | Threads-specific app/token and scopes; `threads_basic`, `threads_content_publish`; insights and reply management require additional scopes. Meta's official collection exposes reply-control options. [M3] | Separate text/image/video/carousel capabilities, permissions, and reply controls. Do not treat a Facebook token as a Threads token. |
| TikTok | Direct Post's intended-use rules exclude clients limited to private/internal use, including a utility for the owner's/team's accounts. An unaudited client has private-viewership/test-user limits. [T1] | **Default to manual handoff for this private self-hosted use case.** Open-source availability alone does not establish eligibility or app approval. An approved broadly available product would need a separate assessment. |

### X cost and retry constraints

Current [X pricing](https://docs.x.com/x-api/getting-started/pricing) is pay-per-use rather than the older subscription tiers: the published table lists $0.015 per post-create request and $0.200 when creating with a URL; post reads are $0.005 per returned resource. Rates can change; the operator must inspect their console, approve costs and set spending limits. This research authorizes no purchases or paid requests. Polling analytics/inbox can cost more than posting. Persist usage timestamps and honor provider rate-limit responses; do not retry an uncertain create blindly.

### YouTube documentation change

The current [videos.insert reference](https://developers.google.com/youtube/data/reference/rest/v3/videos/insert) specifies an upload bucket of 100 calls per day, one unit per upload call. Do not hard-code the historical 1,600-unit figure. The current [video resource reference](https://developers.google.com/youtube/data/reference/rest/v3/videos#status.privacyStatus) says uploads from unverified API projects are not automatically restricted to private viewing; [YouTube Help](https://support.google.com/youtube/answer/7300965) still describes private-lock outcomes for unverified API services. This is a documented inconsistency, not proof of access for any particular project. Inspect the actual project's quota, consent/verification state and video status before claiming readiness. Native `publishAt` requires a private, never-published video; past timestamps can publish immediately. Always set privacy explicitly.

### TikTok handoff

The supported no-API route is to download the media, copy the caption and open TikTok for the owner to finish. Mark it **awaiting manual publication** until the owner records the resulting URL/receipt; a reminder is not a published post. TikTok also documents an [Upload API](https://developers.tiktok.com/docs/en/content-posting-api-reference-upload-video) (`video.upload`, inbox video initialization) that hands editing to the TikTok app. It still needs approved API access; it is not a workaround for absent approval. The documented per-token initialization limit is six requests/minute, and pull-from-URL requires a verified URL prefix/domain.

## Other Buffer networks

The current [Buffer channel matrix](https://support.buffer.com/articles/supported-channels-LM3P7Y4zsp) also lists LinkedIn, Pinterest, Mastodon, Bluesky, Google Business Profile, Substack and Start Pages, with different publishing, analytics and comment-management support. Buffer availability does not grant a self-hoster the same API permissions.

| Network | Practical direct integration and boundary |
| --- | --- |
| Mastodon | Register an app on the selected instance, authorize through OAuth, discover that instance's limits. Status publishing supports text, media, polls, visibility, content warning and language. `Idempotency-Key` prevents duplicate submissions for a limited window (documented up to one hour), so retain local reconciliation after that window. [A1, A2] |
| Bluesky | AT Protocol create-session/refresh-session and repository records are documented. An owner app-password flow can keep credentials server-side; session refresh uses the refresh JWT. A post is limited to 300 graphemes and 3,000 string-length units in the inspected lexicon, with separate rich-text facets and embeds. Generate and persist the record key before creation so a lost response can be checked by URI rather than creating another record. [B1–B4] |
| LinkedIn | OAuth `w_member_social` enables member posting through the self-service Share product. The current Posts API supersedes `ugcPosts` and requires version headers. Organization publishing/read access and member reads have distinct permissions/approval restrictions. Do not promise full inbox/analytics from member publishing permission alone. [L1, L2] |
| Pinterest | Business account and reviewed trial-access application precede app ID/secret access; exact callback matching, OAuth and refresh are documented. Pin/board/media implementations must respect actual trial or standard permissions. Approval is an external blocker, not solved by saving credentials. [P1] |
| Google Business Profile | Project approval, a verified active Business Profile and associated business website are prerequisites. API supports local news/CTA/event/offer posts; product posts are excluded in the guide. Location identity, approval and OAuth are necessary before delivery. [G1, G2] |
| Substack | Buffer lists it, but this investigation did not establish a supported public publishing API suitable for an independent self-hoster. Use manual handoff; do not use reverse-engineered private endpoints. |
| Start Pages | Buffer's own product, not an independent social network. No dependency or cloned proprietary service is required for beweft. |

### Google Business Profile implementation notes

The [create method](https://developers.google.com/my-business/reference/rest/v4/accounts.locations.localPosts/create)
accepts a LocalPost under `accounts/{account}/locations/{location}` using
`business.manage`. The [resource contract](https://developers.google.com/my-business/reference/rest/v4/accounts.locations.localPosts)
distinguishes STANDARD, EVENT and OFFER; events and offers need event dates,
CALL omits a URL, and other calls to action use a URL. Persist the returned
resource name, state and search URL. Media uses a publicly reachable source URL.
An ambiguous create cannot be declared absent from a missing local receipt.

[Project eligibility](https://developers.google.com/my-business/content/prereqs)
requires an active verified profile for at least 60 days, a representative
website and API approval. Saving OAuth credentials does not establish those
conditions. This is a practical adapter candidate with operator qualification;
it is not automatically blocked merely because no account is connected yet.

## Analytics and inbox contract

Treat metrics as observations with provider, account, post, measurement time, period and coverage. Missing, delayed, inaccessible and zero are different values. Store the actual provider receipt; never fabricate engagement to fill a chart. For YouTube, the [Analytics reports endpoint](https://developers.google.com/youtube/analytics/reference/reports/query) documents channel-authorized metrics and its analytics scopes, with an additional `youtube.readonly` requirement noted on the page. A publishing grant alone is insufficient.

Comment/reply access is not a universal DM inbox. Present each adapter's supported interactions, required scopes, last successful sync and unavailable reason. Paginate, checkpoint and deduplicate reads; apply provider rate limits. Reply actions require the same authoritative delivery/audit path as scheduled posts. Availability of an official feature must still be implemented and tested before it is exposed as functional in beweft.

## Implementation checks

- Keep provider apps, authorization grants, destination channels and per-format capabilities distinct.
- Enforce allowed outbound provider hosts; arbitrary instance/media URLs require SSRF protection. Never log tokens, signed upload URLs, cookies or callback codes.
- Persist creation/container/upload IDs and state transitions. An ambiguous write goes to reconciliation, not immediate retry. Use a stable provider idempotency key where supported; otherwise require positive evidence before retry or owner resolution.
- Validate scopes and account identity after authorization. Missing optional analytics permission must not hide working publishing, and missing publishing permission must block sending.
- Show a precise manual path when the API is unavailable. Record owner-confirmed manual completion separately from provider-verified publication.
- Fixture verification proves request/response handling only. Live qualification requires authorized sandbox/test accounts and explicit permission before any public post or paid API request.

## Official source map

- X1: [OAuth 2 + PKCE](https://docs.x.com/fundamentals/authentication/oauth-2-0/authorization-code).
- X2: [Create post](https://docs.x.com/x-api/posts/create-post).
- Y1: [Video upload](https://developers.google.com/youtube/data/reference/rest/v3/videos/insert).
- Y2: [Video resource and status](https://developers.google.com/youtube/data/reference/rest/v3/videos).
- Y3: [Google web-server OAuth](https://developers.google.com/identity/protocols/oauth2/web-server) (fetch timed out in this review; recheck during adapter implementation).
- Y4: [YouTube Analytics query](https://developers.google.com/youtube/analytics/reference/reports/query).
- M1: [Meta-maintained Facebook collection](https://www.postman.com/meta/facebook/documentation/r56bjfd/facebook-api).
- M2: [Meta-maintained Instagram collection](https://www.postman.com/meta/instagram/documentation/6yqw8pt/instagram-api). Search-indexed documentation supplied account types and scopes; direct page rendering was incomplete. [Content publishing reference](https://developers.facebook.com/docs/instagram-platform/content-publishing/) returned HTTP 429. Recheck exact format/scopes/version constraints before implementation or live qualification.
- M3: [Meta-maintained Threads collection](https://www.postman.com/meta/threads/request/u22r2j5/control-who-can-reply). Indexed official content supplied scopes/options; direct rendering later failed. [Threads publishing reference](https://developers.facebook.com/docs/threads/posts/) returned HTTP 429. Do not treat these retrieval limits as verification of every format.
- T1: [TikTok Content Sharing Guidelines](https://developers.tiktok.com/docs/en/content-sharing-guidelines).
- A1: [Mastodon app registration](https://docs.joinmastodon.org/methods/apps/).
- A2: [Mastodon status methods](https://docs.joinmastodon.org/methods/statuses/).
- B1: [Bluesky post lexicon](https://github.com/bluesky-social/atproto/blob/c2b5ce39ebf1c33857595340a87551800689e8fd/packages/api/lexicons/app/bsky/feed/post.json).
- B2: [AT Protocol create record](https://github.com/bluesky-social/atproto/blob/c2b5ce39ebf1c33857595340a87551800689e8fd/lexicons/com/atproto/repo/createRecord.json).
- B3: [Create session](https://github.com/bluesky-social/atproto/blob/c2b5ce39ebf1c33857595340a87551800689e8fd/lexicons/com/atproto/server/createSession.json).
- B4: [Refresh session](https://github.com/bluesky-social/atproto/blob/c2b5ce39ebf1c33857595340a87551800689e8fd/lexicons/com/atproto/server/refreshSession.json).
- L1: [Share on LinkedIn](https://learn.microsoft.com/en-us/linkedin/consumer/integrations/self-serve/share-on-linkedin).
- L2: [Current LinkedIn Posts API](https://learn.microsoft.com/en-us/linkedin/marketing/community-management/shares/posts-api?view=li-lms-2026-10).
- P1: [Pinterest app registration](https://developers.pinterest.com/docs/getting-started/connect-app/).
- G1: [Business Profile prerequisites](https://developers.google.com/my-business/content/prereqs).
- G2: [Business Profile posts](https://developers.google.com/my-business/content/posts-data).
