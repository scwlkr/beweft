# beweft API contract

Additional implemented content and account workflows are specified in [workflows-api.md](workflows-api.md).

Base `/api/v1`. Success `{ "data": ... }`; failure `{ "error": { "code": "...", "message": "..." } }`. Lists return arrays in `data`. IDs are UUID strings; timestamps RFC3339 UTC. JSON requests set `Content-Type: application/json` and `X-Beweft-Request: 1`. Browser calls use cookie credentials. State-changing browser requests must have matching Origin when present; authenticated cookie mutations require the custom header. All endpoints except auth/status, auth/bootstrap and auth/login need authentication. API tokens use `Authorization: Bearer ...`. Read APIs accept valid tokens except token management; writes need the listed scope. Token listing/creation/revocation and logout require an owner cookie session and reject bearer tokens, including admin tokens. Tokens may restrict `brand_ids`; UI sessions have all owner access.

The following stable codes identify correctable rejections. The indicated
validation happens before a durable change, or its transaction is rolled back:

| HTTP | Code | Meaning |
| --- | --- | --- |
| 400 | `post_text_too_long` | Post or channel-variant text exceeds 100,000 bytes. |
| 400 | `unsupported_media_type` | The file signature is not JPEG, PNG, GIF, WebP, MP4 or WebM. |
| 400 | `missing_posting_schedule` | A queued channel has no posting rules. |
| 409 | `posting_slots_unavailable` | No posting-rule slot is free in the next 60 days. |
| 409 | `post_version_conflict` | The submitted post version is stale. |
| 400 | `invalid_cursor` | A list cursor is malformed or incompatible with its filters or scope. |
| 400 | `invalid_page_limit` | The list limit is outside 1–10. |

Existing `unauthorized` (401) and `forbidden` (403) codes indicate authentication
or access denial. Other failures retain their existing codes. A transport
failure, interrupted response or unknown error after a write does not establish
its outcome.

## Bounded list inspection

GET `/posts`, `/media`, and `/publications` accept optional `limit` (1–10) and
`cursor`. A request with `limit` returns `{data:[...],next_cursor:string|null}`;
the cursor is null on the last page. Supply the returned cursor with a limit and
the same route, filters, view, and authorized brand scope. Malformed or
incompatible cursors return 400. A cursor is opaque and limited to 2048
characters; it does not grant access. Every page applies authorization and
brand filters before pagination. Pages use `created_at DESC, id DESC`, with
the UUID breaking timestamp ties. They are live views rather than snapshots;
refresh from the beginning after concurrent changes. Without `limit`, existing
list order and response shape remain unchanged for the web application.

- `/posts` additionally accepts `status=draft` and `view=summary`. The summary
  is `{id,brand_id,title,text,media_ids,version,status,created_at,updated_at}`;
  variants and options are omitted. GET `/posts/:id` returns the full post.
- `/media` returns the normal media record on each page.
- `/publications` additionally accepts `group=queue|history`, `view=summary`,
  and `id=UUID`. Queue means queued/sending; history means every other state.
  The summary is `{id,post_id,brand_id,channel_id,channel_name,provider,post_version,scheduled_at,scheduling_mode,state,created_at,updated_at}`;
  payload and external/error fields are omitted. GET
  `/publications?id=UUID&limit=1` without `view=summary` reads one full,
  authorized record in the usual `data` array.

The generated OpenAPI and TypeScript query annotations for these additions
will be refreshed with the core integration; this prose and the running routes
describe the current change.

## Authentication
- GET `/auth/status` → `{initialized:boolean}`.
- POST `/auth/bootstrap` `{setup_secret,email,password}` → `{id,email}` + session cookie. Password minimum 12 characters. Setup secret is `BEWEFT_SETUP_SECRET` on server. Only available before first owner.
- POST `/auth/login` `{email,password}` → `{id,email}` + session cookie.
- GET `/auth/me` → `{id,email}`.
- POST `/auth/logout` `{}` → `{ok:true}`. Owner cookie session only.
- GET `/tokens` → token metadata array. Owner cookie session only.
- POST `/tokens` `{name,scopes:["read","drafts:write","publishing:write","admin"],brand_ids?:[]}` → `{id,token,name,scopes,brand_ids}`. Secret returned once. Owner sessions only.
- DELETE `/tokens/:id` → `{ok:true}`. Owner cookie session only.

## Brands and connections
- GET `/brands`; POST `/brands` `{name,color?:"#8b5cf6"}` → brand. Admin write.
- Brand `{id,name,color,archived_at,created_at}`. Archived brands are hidden unless include_archived=true.
- GET `/providers` → capability descriptors. Provider status truthfully distinguishes implementation, configuration, execution, and verification; unavailable adapters cannot be connected.
- GET `/channels?brand_id=UUID`; POST `/channels` `{brand_id,name,provider:"manual",timezone?:"America/Chicago"}` → channel. Admin write. `fixture` is accepted only with explicit server opt-in and remains labeled fixture.
- PATCH `/channels/:id` `{name?,paused?,timezone?}` → channel. Admin write.
- Channel `{id,brand_id,name,provider,timezone,paused,capabilities,created_at}`. Manual channels require owner completion; they never claim automatic publication.

## Ideas and drafts
- GET `/ideas?brand_id=UUID`; POST `/ideas` `{brand_id,title,text?:"",color?:"lavender"}` → idea. PATCH `/ideas/:id` `{title?,text?,color?}`; DELETE `/ideas/:id`.
- Idea `{id,brand_id,title,text,color,created_at,updated_at}`.
- GET `/posts?brand_id=UUID`; GET `/posts/:id`.
- POST `/posts` `{brand_id,title?:"",text,media_ids?:[],options?:{},variants?:[{channel_id,text?,media_ids?,options?}]}` → post.
- PATCH `/posts/:id` `{version:number,title?,text?,media_ids?,variants?,options?}` → post. Version required for conflict protection. Existing publication snapshots do not change; edit and schedule a new revision explicitly.
- DELETE `/posts/:id` deletes only drafts with no publications. Other posts remain as history.
- Post `{id,brand_id,title,text,media_ids,variants,options,version,status,created_at,updated_at}`. Status is draft/scheduled/published/attention derived from publications.
- Idea/post/media writes require `drafts:write`.

## Media
- GET `/media?brand_id=UUID` → media array.
- GET `/media/limits` → `{max_upload_bytes,quota_bytes,used_bytes,reserved_bytes,available_bytes}`. Storage quota defaults to 10 GiB (`BEWEFT_MEDIA_QUOTA_BYTES`); active uploads reserve capacity atomically. Uploads have a 15-minute deadline.
- POST `/media?brand_id=UUID` multipart field `file` → media. Maximum `BEWEFT_MAX_UPLOAD_BYTES` (default 256 MiB), streamed to persistent storage; JPEG/PNG/WebP/GIF/MP4/WebM only. Authentication plus custom header applies to multipart too.
- GET `/media/:id/file` → authenticated file bytes.
- DELETE `/media/:id` → `{ok:true}`; media referenced by draft or publication cannot be removed.
- Media `{id,brand_id,name,mime,size,url,alt_text,created_at}`; `url` is the authenticated file endpoint.

## Scheduling and outcomes
- GET `/schedules?channel_id=UUID` → posting rules array.
- POST `/schedules` `{channel_id,days:[1,2,3,4,5],time:"09:00",timezone:"America/Chicago"}` → rule. Monday=1, Sunday=7. DELETE `/schedules/:id`. Each rule `{id,channel_id,days,time,timezone,created_at}`.
- POST `/posts/:id/schedule` `{version:number,channel_ids:[UUID],scheduled_at?:RFC3339}` → publication array. Version must match the locked draft; a stale version returns 409 without scheduling. Omit scheduled_at to reserve each channel's next available posting-rule slot. Requires `publishing:write`. Exact dates must be future (up to five seconds grace); all variants snapshot atomically. Optional query `view=summary` returns the same publication summary fields described under bounded list inspection, omitting payload and external/error fields; the default response retains full publications. An unsupported view is rejected before scheduling. The view selects response representation and is not part of scheduling intent.
- GET `/publications?brand_id=UUID&channel_id=UUID` → publication array, earliest first. Optional filters.
- Publication `{id,post_id,brand_id,channel_id,channel_name,provider,post_version,scheduled_at,scheduling_mode,state,payload,external_id,external_url,last_error,created_at,updated_at}`. `payload` contains immutable `{text,media_ids,title,options}`. States: queued, sending, manual_required, published, failed, uncertain, canceled. Fixture success uses `fixture_complete`, never published.
- GET `/publications/:id/attempts` → durable attempts array with state and errors.
- POST `/publications/:id/cancel` `{}` → publication; only queued/manual_required/failed may cancel.
- POST `/publications/:id/manual-complete` `{external_url?:URL,note?:string}` → publication. Manual_required only; owner attestation stored, no verified provider receipt implied.
- POST `/publications/:id/reconcile` `{resolution:"published"|"not_published",external_id?:string,external_url?:URL,note:string}` → publication. Uncertain only. Published result is owner-confirmed reconciliation; not_published becomes failed and explicitly retryable.
- POST `/publications/:id/retry` `{}` → publication. Failed only, resets schedule to now and enqueues a new attempt. No automatic uncertain retries.
- POST `/queue/reorder` `{channel_id,publication_ids:[UUID...]}` → publication array. Complete list of that channel's `state=queued, scheduling_mode=queue` publications required. Exact-date publications are excluded and keep their dates. Preserves allocated queue slots and changes ordering transactionally.
- POST `/publications/:id/reschedule` `{scheduled_at:RFC3339}` → publication. Queued/manual_required only, optimistic state checks. Sets scheduling_mode to exact.
- Publication and schedule mutations require `publishing:write`.

## Health and honest unsupported surfaces
- GET `/jobs/health` → `{worker_last_seen:timestamp|null,worker_healthy:boolean,queued:number,uncertain:number,manual_required:number,oldest_due:timestamp|null}`.
- GET `/analytics?brand_id=UUID` → `{availability:"unavailable",reason:string,summary:{scheduled:number,published:number,attention:number},metrics:[]}`. Summary counts are local workflow counts, not network performance metrics.
- GET `/inbox?brand_id=UUID` → `{availability:"unavailable",reason:string,items:[]}`.
- GET `/healthz` outside API → `{status:"ok"}` after database check.
- GET `/api-docs/openapi.json` → OpenAPI document.

Provider adapter integration routes will be added in a separate change. This contract does not promise real provider OAuth or publishing in the foundation.

## Direct provider connection slice

These routes belong to the provider integration change; check installed routes/version before use. Provider app configuration and connection flows require an owner browser session, not an API token. Secret values are accepted once and never returned. Provider grants are encrypted server-side. Network access in these flows is real authorization/identity lookup; provider costs can apply.

- GET `/provider-apps` → `[{provider,client_id,instance_url:string|null,configured:true,updated_at}]`.
- PUT `/provider-apps/:provider` `{client_id,client_secret?,instance_url?}` → `{provider,client_id,instance_url,configured:true,callback_url}`. Omitted secret preserves the saved value; empty secret is rejected. Bluesky can use empty client ID/secret and defaults its instance to `https://bsky.social`. Mastodon/Bluesky instances must match server `BEWEFT_ALLOWED_PROVIDER_ORIGINS` and resolve only to public IP addresses. Other providers reject instance URLs.
- POST `/connections/:provider/start` `{brand_id,reconnect_channel_id?}` → `{authorization_url,flow_id}` plus a separate HttpOnly SameSite=Lax OAuth correlation cookie. Navigate to `authorization_url`. The existing login cookie is retained. Flow expiry is ten minutes; one concurrent browser flow is supported.
- GET `/connections/:provider/callback?state=...&code=...` is public but validates single-use state, correlation cookie and the original still-active owner session. It stores encrypted destination candidates and redirects to `/settings?connection_flow=UUID`. Declined consent redirects with `connection_error=consent_declined`. Other failures return the normal safe error envelope.
- GET `/connection-flows/:id` → `{id,brand_id,provider,destinations:[{id,name,username:string|null}]}`. Requires the same owner session which started the flow.
- POST `/connection-flows/:id/complete` `{destination_id,timezone?:"UTC"}` → channel. Select one returned destination. Reconnect refuses a different external identity. Existing schedules/history remain attached to the original channel; a disconnected/paused channel stays paused until the owner resumes it.
- POST `/connections/bluesky` `{brand_id,identifier,app_password,reconnect_channel_id?,timezone?}` → channel after a real provider identity check. App password is not retained; resulting session grants are encrypted.
- POST `/channels/:id/disconnect` `{}` → `{disconnected:true,provider_revoked:boolean,next_action}`. Stops queued deliveries, pauses the channel and clears credentials. An active delivery must resolve first. If remote revocation is unavailable/fails, `next_action` directs the owner to the provider's connected-app settings.
- GET `/providers` now describes installed adapters with `{id,name,formats,oauth,analytics,inbox,reply,implementation,configuration,execution,verification,reason}`. Live verification remains `unverified_live` until separately qualified. A connection changes channel verification to `identity_verified_publishing_unverified`, not live publishing verified.
- Channel additionally exposes `{connection_identity:string|null,connection_status:"manual"|"connected"|"reconnect_required"|"disconnected",connection_error:string|null,connected_at:timestamp|null}`. Credentials never appear in channel responses.

Publishing options are immutable per-channel snapshot fields under `options`. Current adapter fields: YouTube `{privacy:"private"|"unlisted"|"public",made_for_kids:boolean,contains_synthetic_media?:boolean,category_id?:string}`; Mastodon `{visibility?:"public"|"unlisted"|"private"|"direct",content_warning?:string,sensitive?:boolean}`; Threads `{reply_control?:"everyone"|"accounts_you_follow"|"mentioned_only"}`; Pinterest `{board_id:string,link?:string}`. The adapter rejects unsupported media formats instead of silently stripping them. Provider descriptions list current format limitations.

### Provider observations and recovery

- POST `/channels/:id/sync` `{allow_metered?:boolean}` → `{metrics_observed,inbox_items,coverage:"partial_recent_page",errors:string[],note}`. Requires `publishing:write`; X refuses reads unless `allow_metered:true` explicitly acknowledges costs. Fetches observations for at most ten local published posts and one recent inbox page (Meta: one page for each of at most ten local posts). No automatic paid polling.
- GET `/analytics` keeps the local `summary` and returns `availability:"unavailable"|"partial"`, `reason`, and `metrics:[{publication_id,channel_id,provider,text,external_url,observed_at,metrics:[{name,value}]}]`. Provider metrics are never synthesized from local workflow counts.
- GET `/inbox` returns `availability:"unavailable"|"partial"`, `reason`, `items:[{id,channel_id,channel_name,provider,item:{id,parent_id,author,text,url,created_at},read_at,synced_at,can_reply}]`. Render text as text, never raw HTML; Mastodon content may contain markup.
- POST `/inbox/:id/read` `{}` → `{ok:true}`.
- POST `/inbox/:id/reply` `{text,request_id:UUID}` → durable reply `{id,inbox_id,owner_id,text,state,receipt,error,created_at,updated_at}`. Reuse the same request ID when checking a lost response. The server never resends that ID. `state` is sending/published/failed/uncertain. Reply writes need `publishing:write` and can have real public effects.
- POST `/publications/:id/provider-reconcile` `{}` → updated publication if a provider receipt conclusively proves completion, otherwise `{state:"uncertain",reason}`. Current automatic probes cover YouTube resumable upload completion and Bluesky deterministic record lookup. Absence of proof never marks not-published or retries automatically.
- GET `/provider-media/:id?token=...` is an expiring provider-fetch capability URL, issued server-side for supported media publication. It must not be logged or shared. It is not the authenticated media library URL.

## Generated contracts

Core JSON handlers deserialize the Rust request types in `crates/server/src/contracts/requests.rs`; OpenAPI derives their required fields, UUID/date formats, and response structures from Rust. Generate the OpenAPI artifact and frontend types from this same source:

```sh
cargo run -q -p beweft-server --bin beweft-contract > docs/openapi.json
cargo run -q -p beweft-server --bin beweft-contract -- --typescript > web/src/lib/api-contract.generated.ts
```

`ApiOperations` maps each method/path to its typed request, response envelope, and query parameters. Binary media responses are explicitly represented as bytes/Blob. Invalid typed JSON returns 422; business validation returns 400. The HTTP integration runner checks actual response bodies against the served schemas.

Queue-slot daylight-saving policy: a nonexistent local time is skipped that day; a repeated local time uses the earlier occurrence only. Existing publications created before scheduling modes were recorded are conservatively retained as exact dates. Explicit rescheduling pins a publication to an exact date.

Local server binding defaults to `127.0.0.1:3000`; container deployments explicitly set `BEWEFT_BIND=0.0.0.0:3000`. HTTP request spans record method and path only, omitting OAuth query parameters. `BEWEFT_FIXTURE_DELAY_MS` is an opt-in fixture-only fault-injection delay (capped at four minutes), used to exercise real worker interruption/restart and independent heartbeat behavior.

Provider safety additions:

- Channel `capabilities.permissions` is `{source,reported_scopes,actions,note}`. Each action (`publish`, `media`, `metrics`, `inbox`, `reply`) is `{status:"granted"|"missing"|"unknown",required_scopes:string[],missing_scopes:string[]|null}`. Capability implementation and observed scope grants are distinct. Unknown is not verified permission. Missing required scope blocks the corresponding operation.
- Inbox `item.reply_context` is nullable `{visibility:string,recipients:{id:string,acct:string}[]}`. For Mastodon, private/direct sources produce direct replies only to these participants. Public/unlisted retain their visibility. The reply request body remains `{text,request_id}`; added `@` mentions are rejected.
- `GET /publications/{id}/provider-status` returns `{session_expired:boolean,reset_allowed:boolean,reason:string|null}` without exposing resumable URLs or credentials.
- `POST /publications/{id}/provider-reconcile` additionally returns `{state:"uncertain",session_expired:true,reason}` for an expired YouTube session. This is not proof of nonpublication.
- `POST /publications/{id}/reset-expired-session` requires an owner browser session, failed publication, expired-session receipt and a later owner `not_published` reconciliation. Returns `{state:"failed",session_reset:true,next_action:string}`; use the existing explicit retry action afterward. Otherwise 409. Reset never schedules a replacement automatically.

Worker integration: `provider_bridge::publish` requires the immutable publication JSON plus `attempt_id` containing the UUID already persisted by its current worker claim. All worker completion data, including provider IDs/URLs, must be committed through the same attempt-fenced completion transaction. Checkpoints are written only while that attempt remains sending with an unexpired lease.

Durable reply recovery:

- `GET /inbox/{id}/replies` is read-only and returns an owner-scoped array of durable reply attempts `{id,inbox_id,text,state,receipt,error,created_at,updated_at}`, newest first (up to 100). Sending attempts past their stored delivery deadline are projected as `uncertain`; reading never sends or mutates a reply. Load this before enabling a new reply, including after reopening a conversation or switching browsers.
- `POST /inbox/{id}/reply` reuses the supplied UUID for recovery. A fresh UUID receives 409 while **any** sending/uncertain attempt exists for that inbox item. The guard and claim are atomic under the channel lock, so concurrent tabs cannot bypass it.
- `POST /inbox/{id}/replies/{request_id}/resolve` requires an owner browser session and `{resolution:"published"|"not_published",note:string,external_url?:string}`. It accepts uncertain or expired sending attempts, records an owner attestation in the receipt and audit log, and returns the durable reply row. Published becomes `published`; not-published becomes `failed`. Resolution never sends a request to the network. A new reply after not-published requires another explicit user action.

Reply-history watermark: a new `POST /inbox/{id}/reply` includes `expected_last_request_id: UUID|null`, taken from the first row of the latest acknowledged `GET /inbox/{id}/replies`, or null for empty history. Omitted means null and therefore requires empty history. Under the same claim lock, the server compares this watermark with the latest durable attempt before claiming a fresh UUID. A mismatch returns HTTP 409 with `error.code="reply_history_changed"`; refresh history and require explicit confirmation, never automatically retry with another UUID. Reusing an already stored request UUID returns its original result before the watermark check. History uses a durable insertion sequence, so lock waits and clock changes cannot make a newer claim appear older. The unresolved-attempt guard still applies independently.

Reply claim timing: new attempts store `created_at=clock_timestamp()` and an internal `delivery_deadline` 90 seconds after the actual claim. HTTP delivery uses that same absolute deadline. Lock waits and credential refresh do not count as time in flight; GET recovery, disconnect, new-reply guards and owner resolution all use the stored deadline, not the display timestamp. The public reply-row shape is unchanged.
