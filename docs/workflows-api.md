# Content workflow API additions

All paths below are relative to `/api/v1`, with the existing `{data:...}` envelope, cookie/header protection, token scopes and brand restrictions. These additions are the current frontend implementation contract.

## Ideas and groups

Ideas gain `status: "inbox"|"in_progress"|"ready"|"archived"` (default inbox), `group_id: UUID|null`, and `tags: string[]` (maximum 20, each 40 characters). Existing POST/PATCH `/ideas` accept these fields. PATCH `group_id:null` clears a group. GET `/ideas` accepts optional `brand_id`, `status`, `group_id` and `tag` filters.

- GET `/idea-groups?brand_id=UUID` → groups array.
- POST `/idea-groups` `{brand_id,name,color?:"lavender",position?:0}` → group.
- PATCH `/idea-groups/:id` `{name?,color?,position?}` → group.
- DELETE `/idea-groups/:id` → `{ok:true}`; ideas are retained and become ungrouped.
- Group `{id,brand_id,name,color,position,created_at}`. Writes require drafts:write.

## Reusable templates

- GET `/template-starters` → original starter array `{id,name,title,text}`; no records are created automatically.
- GET `/templates?brand_id=UUID` → saved templates array.
- POST `/templates` `{brand_id,name,title?:"",text,media_ids?:[],variants?:[],options?:{}}` → template.
- PATCH `/templates/:id` `{version,name?,title?,text?,media_ids?,variants?,options?}` → template. Optimistic version required.
- DELETE `/templates/:id` → `{ok:true}`.
- POST `/templates/:id/apply` `{variables?:{key:"value"}}` → new unscheduled Post in the template's brand. Literal `{{key}}` substitutions apply to title/text/variant text; unknown placeholders remain editable in the draft. `{{brand}}` defaults to the current brand name. This never schedules content.
- Template `{id,brand_id,name,title,text,media_ids,variants,options,version,created_at,updated_at}`. Writes/apply require drafts:write. Referenced template media cannot be deleted.

## Media descriptions and manual guidance

- Media responses add `alt_text:string` (default empty) and `metadata_version:number` (initially 1). GET `/media/:id` reads the current authorized asset.
- PATCH `/media/:id` `{alt_text:string,metadata_version:number}` → media with incremented metadata version. The required version must match the current record; a stale version returns 409 `media_version_changed` without updating it. Maximum 2,000 characters; NUL is rejected with 400 `media_description_contains_nul`. Requires drafts:write. Newly scheduled snapshots freeze per-media descriptions in `payload.media_metadata:[{id,alt_text}]`; existing queued content is unchanged.
- GET `/manual-platforms` → allowlisted `{id,name,compose_url:string|null,guidance:string}` entries.
- Manual channel POST/PATCH accepts `target_platform` (default `generic`), one of generic/x/youtube/facebook/instagram/threads/tiktok/bluesky/mastodon/linkedin/pinterest. Nonmanual channels cannot set this field. Channel responses include `target_platform:string|null` and `manual_guidance:{id,name,compose_url,guidance}|null`. Links are server-owned HTTPS destinations, never user-supplied redirect URLs.

## Brands

- GET `/brands?include_archived=true` includes archived brands; default hides them. Brand responses add `archived_at:timestamp|null`.
- PATCH `/brands/:id` `{name?,color?,archived?:boolean}` → brand. Admin scope and brand access required. Archive preserves all data, pauses channels and blocks new scheduling/claims; rejects while a delivery is sending. Unarchive keeps channels paused until explicitly resumed.

## Queue edits and priority

- GET `/publications/:id/replacement-status` → `{original:PublicationSummary,replacement:PublicationSummary|null}`. The replacement, if present, is the exact audit-linked successor. Null does not prove a pending replacement ended. Access follows the original's brand; missing or inaccessible originals return 404.
- POST `/publications/:id/replace` `{post_version:number,publication_post_version:number,scheduled_at:RFC3339}` → `{previous_publication_id,publication}`. Uses current saved draft for the same post/channel. All three expected values must match locked records and the original publication must be queued. Atomically cancels the old snapshot, creates a fresh immutable snapshot, and preserves its exact time and scheduling mode. Old history remains; sending/manual-required/outcome records cannot be replaced.
- POST `/publications/:id/prioritize` `{scheduled_at:RFC3339}` → reordered publication array. The expected timestamp must match. Moves a queued queue-slot publication to the first existing queue slot, shifting other queue-slot records; exact-date records never move. Publishing scope required.

## Owner security

- POST `/auth/password` `{current_password,new_password}` → `{ok:true}`. Owner session only, minimum 12-character new password. Replaces password and revokes every session; frontend returns to sign-in. Existing API tokens remain manageable separately.
- GET `/auth/sessions` → `{id,created_at,expires_at,current}[]`; id is an opaque session UUID, never the cookie secret/hash.
- DELETE `/auth/sessions/:id` → `{ok:true}`; revokes that session. Owner session only. Revoking the current session signs it out.
- POST `/auth/sessions/revoke-others` `{}` → `{ok:true}`. Keeps current session and revokes all other owner sessions.
