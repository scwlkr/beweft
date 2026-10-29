# Google Business Profile API research

Checked 2026-10-29 against Google's documentation. These are documented API
capabilities and prerequisites, not evidence of a working account or live
publication in beweft. No account was connected and no provider request or post
was made for this research.

## Access and destination discovery

- Google requires project-level Business Profile API approval. Its current
  prerequisites include a verified profile active for at least 60 days, a
  website representing that business, and an Organization account. Enable
  the relevant APIs in the approved project and configure an OAuth web client.
  [Prerequisites](https://developers.google.com/my-business/content/prereqs),
  [basic setup](https://developers.google.com/my-business/content/basic-setup).
- Account Management lists the authenticated user's accounts, with pagination;
  Business Information lists each account's locations, also with pagination
  and a required read mask. Both use the `business.manage` OAuth scope.
  [Accounts list](https://developers.google.com/my-business/reference/accountmanagement/rest/v1/accounts/list),
  [locations list](https://developers.google.com/my-business/reference/businessinformation/rest/v1/accounts.locations/list).
  Business Information identifies a location as `locations/{locationId}`;
  the v4 Local Posts endpoint also needs the containing `accounts/{accountId}`.
- The Business Information `canOperateLocalPost` metadata field is deprecated
  and no longer populated. An account or location listing alone does not prove
  local-post eligibility. [Location reference](https://developers.google.com/my-business/reference/businessinformation/rest/v1/accounts.locations).

## Posts and outcomes

- The Local Posts API documents `STANDARD`, `EVENT`, and `OFFER`; product posts
  cannot currently be created through this API. Event and offer posts require
  event information; an offer may add a coupon, redemption URL, and terms.
  [LocalPost reference](https://developers.google.com/my-business/reference/rest/v4/accounts.locations.localPosts),
  [posts guide](https://developers.google.com/my-business/content/posts-data).
- The documented call-to-action types are `BOOK`, `ORDER`, `SHOP`,
  `LEARN_MORE`, `SIGN_UP`, and `CALL`; `CALL` leaves the URL unset. The
  reference says call-to-action is ignored for offers, even though an example
  in the posts guide combines them. Follow the reference rather than silently
  presenting an offer CTA as applied. A local-post media item supports
  `sourceUrl`; the media reference describes photo support for this API
  version. [LocalPost reference](https://developers.google.com/my-business/reference/rest/v4/accounts.locations.localPosts),
  [media reference](https://developers.google.com/my-business/reference/rest/v4/accounts.locations.media).
- Create returns a LocalPost with a resource name. Its states include
  `PROCESSING` and `SCHEDULED`, which are not yet appearing in search;
  `LIVE` and `RECURRING`, which are appearing; and `REJECTED`. A successful
  create is therefore not itself proof that a post is visible. The named post
  can be read with GET. [Create](https://developers.google.com/my-business/reference/rest/v4/accounts.locations.localPosts/create),
  [states](https://developers.google.com/my-business/reference/rest/v4/accounts.locations.localPosts),
  [get](https://developers.google.com/my-business/reference/rest/v4/accounts.locations.localPosts/get).
- Google's photo guidance gives JPEG/PNG, 10 KB–5 MB, and at least 250 pixels
  on each side. It also states that a business must be verified for media to
  appear. Provider policy and actual eligibility can impose further limits.
  [Photo guidance](https://support.google.com/business/answer/6123536).

## Reads beyond post status

Google documents a paginated review list and an update-reply operation. A
reply update can replace an existing response, so it needs explicit product
and recovery semantics rather than being treated as an append-only comment.
[Reviews list](https://developers.google.com/my-business/reference/rest/v4/accounts.locations.reviews/list),
[update reply](https://developers.google.com/my-business/reference/rest/v4/accounts.locations.reviews/updateReply).
The Local Posts insights endpoint documents search views and call-to-action
clicks for named posts. Its basic metrics request has a maximum 18-month
range, and recent data can be missing. Any implementation must label its
actual time window and preserve missing values as unavailable, not zero.
[Post insights](https://developers.google.com/my-business/reference/rest/v4/accounts.locations.localPosts/reportInsights),
[metric definitions](https://developers.google.com/my-business/reference/rest/v4/Metric),
[basic metrics request](https://developers.google.com/my-business/reference/rest/v4/BasicMetricsRequest).

## Shared Google authorization

Google documents that revocation removes all OAuth scopes granted to a Cloud
project and invalidates access and refresh tokens for all client IDs in that
project. A client ID alone is not proof of which other configured clients or
accounts share that project. Disconnect and recovery must therefore avoid
invalidating a newer YouTube or Business Profile authorization during an older
access removal. [Google OAuth revocation](https://developers.google.com/identity/protocols/oauth2/web-server#tokenrevoke),
[cross-client identity](https://developers.google.com/identity/protocols/oauth2/cross-client-identity).

Google notes that fake production listings are not a safe test path and
recommends mock responses for such tests. A fixture demonstrates the local
protocol only; project approval, account eligibility, media ingestion, and
actual publication remain unverified until separately qualified.
[Business Profile API FAQ](https://developers.google.com/my-business/content/faq).

## Draft implementation contract

This section is a design checkpoint, not a description of implemented
behavior. The shared Google-family gate and stable location identity have
initial architectural agreement; the client warning and handling of app
configuration during pending removal still need coordinated review.

- Conservatively use one Google authorization family for YouTube and
  Business Profile, regardless of configured client ID or account. Reuse the
  existing YouTube fence and generation so pending removals and older YouTube
  authorization flows retain their meaning. Serialize callback exchange,
  connection completion, app changes, and disconnect with the family gate.
  No app-issued Google revoke should be sent.
- Before manual removal, lock and inspect in-flight work across both providers;
  mark other affected Google channels as needing reconnection while preserving
  their credentials and queues. A single pending removal blocks both
  providers until the owner attests that intended access removal is finished.
  The client must explain the cross-provider effect, current and prior Google
  app projects, and relevant Google accounts. Owner attestation is not
  provider proof. Consider rejecting app edits while removal is pending; that
  policy and wording remain for review. Older running app versions do not
  understand the shared family, so stop old API and worker processes before
  enabling the new adapter.
- Select the account/location pair returned by Google and retain the selected
  account parent in encrypted connection credentials. Normalize the channel's
  stable destination identity to `locations/{locationId}` so a change in the
  account managing that location does not silently change the destination.
  Reject malformed resource names and incomplete pagination rather than
  presenting a partial list as complete.
- Save the returned LocalPost name before a later status request. A lost create
  response is uncertain and must never trigger an automatic second create.
  Only `LIVE` or `RECURRING` is positive publication evidence; `REJECTED` is a
  failure, while `PROCESSING`, `SCHEDULED`, missing or unknown status, and an
  inconclusive read require continued observation or reconciliation. A local
  queued state currently permits cancellation and content replacement, so it
  must not be used as if a post already created at Google were still unsent.
  One safe initial approach is bounded polling followed by an uncertain state
  and read-only reconciliation using the saved resource name. Restart,
  cancellation, disconnect, and explicit retry must preserve that boundary.
- Remote-fetched media requires the operator's actual configured public HTTPS
  origin and an expiring media capability URL. Loopback is for isolated
  fixtures only; media ingestion timing and live behavior remain unverified.
