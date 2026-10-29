# beweft API contract

Base `/api/v1`. Success `{ "data": ... }`; failure `{ "error": { "code": "...", "message": "..." } }`. Lists return arrays in `data`. IDs are UUID strings; timestamps RFC3339 UTC. JSON requests set `Content-Type: application/json` and `X-Beweft-Request: 1`. Browser calls use cookie credentials. State-changing browser requests must have matching Origin when present; authenticated cookie mutations require the custom header. All endpoints except auth/status, auth/bootstrap and auth/login need authentication. API tokens use `Authorization: Bearer ...`. Read APIs accept any valid token; writes need listed scope. Tokens may restrict `brand_ids`; UI sessions have all owner access.

## Authentication
- GET `/auth/status` → `{initialized:boolean}`.
- POST `/auth/bootstrap` `{setup_secret,email,password}` → `{id,email}` + session cookie. Password minimum 12 characters. Setup secret is `BEWEFT_SETUP_SECRET` on server. Only available before first owner.
- POST `/auth/login` `{email,password}` → `{id,email}` + session cookie.
- GET `/auth/me` → `{id,email}`.
- POST `/auth/logout` `{}` → `{ok:true}`.
- GET `/tokens` → token metadata array (admin scope).
- POST `/tokens` `{name,scopes:["read","drafts:write","publishing:write","admin"],brand_ids?:[]}` → `{id,token,name,scopes,brand_ids}`. Secret returned once. Owner sessions only.
- DELETE `/tokens/:id` → `{ok:true}`.

## Brands and connections
- GET `/brands`; POST `/brands` `{name,color?:"#8b5cf6"}` → brand. Admin write.
- Brand `{id,name,color,created_at}`.
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
- POST `/media?brand_id=UUID` multipart field `file` → media. Maximum 25 MiB; JPEG/PNG/WebP/GIF/MP4/WebM only. Authentication plus custom header applies to multipart too.
- GET `/media/:id/file` → authenticated file bytes.
- DELETE `/media/:id` → `{ok:true}`; media referenced by draft or publication cannot be removed.
- Media `{id,brand_id,name,mime,size,url,created_at}`; `url` is the authenticated file endpoint.

## Scheduling and outcomes
- GET `/schedules?channel_id=UUID` → posting rules array.
- POST `/schedules` `{channel_id,days:[1,2,3,4,5],time:"09:00",timezone:"America/Chicago"}` → rule. Monday=1, Sunday=7. DELETE `/schedules/:id`. Each rule `{id,channel_id,days,time,timezone,created_at}`.
- POST `/posts/:id/schedule` `{channel_ids:[UUID],scheduled_at?:RFC3339}` → publication array. Omit scheduled_at to reserve each channel's next available posting-rule slot. Requires `publishing:write`. Exact dates must be future (up to five seconds grace); all variants snapshot atomically.
- GET `/publications?brand_id=UUID&channel_id=UUID` → publication array, earliest first. Optional filters.
- Publication `{id,post_id,brand_id,channel_id,channel_name,provider,post_version,scheduled_at,state,payload,external_id,external_url,last_error,created_at,updated_at}`. `payload` contains immutable `{text,media_ids,title,options}`. States: queued, sending, manual_required, published, failed, uncertain, canceled. Fixture success uses `fixture_complete`, never published.
- GET `/publications/:id/attempts` → durable attempts array with state and errors.
- POST `/publications/:id/cancel` `{}` → publication; only queued/manual_required/failed may cancel.
- POST `/publications/:id/manual-complete` `{external_url?:URL,note?:string}` → publication. Manual_required only; owner attestation stored, no verified provider receipt implied.
- POST `/publications/:id/reconcile` `{resolution:"published"|"not_published",external_id?:string,external_url?:URL,note:string}` → publication. Uncertain only. Published result is owner-confirmed reconciliation; not_published becomes failed and explicitly retryable.
- POST `/publications/:id/retry` `{}` → publication. Failed only, resets schedule to now and enqueues a new attempt. No automatic uncertain retries.
- POST `/queue/reorder` `{channel_id,publication_ids:[UUID...]}` → publication array. Complete list of that channel's queued publications required. Preserves allocated times and changes ordering transactionally.
- POST `/publications/:id/reschedule` `{scheduled_at:RFC3339}` → publication. Queued/manual_required only, optimistic state checks.
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
