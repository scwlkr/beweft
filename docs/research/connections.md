# Bring your own connections: source review

Reviewed 2026-10-29. This is a source-and-documentation investigation, not a claim that any account was connected or published. No third-party code was copied. Runtime connection flows were not exercised here; the Buffer runtime investigation is recorded separately.

## Projects and license boundaries

| Project | Inspected revision | License | Implication for beweft |
| --- | --- | --- | --- |
| [Postiz](https://github.com/gitroomhq/postiz-app/tree/64f00fd6d0e9dfd55d202ff83983e7576e08bc2e) | `64f00fd6d0e9dfd55d202ff83983e7576e08bc2e` | [AGPL-3.0](https://github.com/gitroomhq/postiz-app/blob/64f00fd6d0e9dfd55d202ff83983e7576e08bc2e/LICENSE) | Learn behaviors and API contracts; do not transplant implementation into the MIT project. |
| [OpenPost](https://github.com/getopenpost/openpost/tree/03ba67e433f5a7e874909f406854453df6120765) | `03ba67e433f5a7e874909f406854453df6120765` | [AGPL-3.0](https://github.com/getopenpost/openpost/blob/03ba67e433f5a7e874909f406854453df6120765/LICENSE) | Same clean implementation boundary. |
| [Mixpost Lite](https://github.com/inovector/mixpost/tree/df57648b866310446703f5294350552b62735df5) | `df57648b866310446703f5294350552b62735df5` | [MIT](https://github.com/inovector/mixpost/blob/df57648b866310446703f5294350552b62735df5/LICENSE.md) | Reuse would require its copyright/license notice; no reuse was needed. Lite findings do not describe commercial Mixpost editions. |

## Comparison

| Concern | Postiz | OpenPost | Mixpost Lite |
| --- | --- | --- | --- |
| Operator setup | Per-provider environment variables; frontend/public URL affects redirects. Hidden and migrated provider switches exist. [P1] | Instance-admin app registry or environment/file JSON; environment wins and is shown read-only. Guide explicitly distinguishes app configuration from workspace account authorization. [O1, O2] | Services screen stores app configuration, with distinct configured/active states and a link from missing-service alerts. [M1, M2] |
| OAuth and selection | Provider-specific authorization implementation. Controller puts organization, verifier and reconnect target behind expiring state in Redis. X uses OAuth 1.0a rather than assuming every provider refreshes OAuth 2 tokens. [P2, P3] | Callback guides show an exact URI. Single-use database state expires after ten minutes and includes user, workspace and provider. Destination selection is part of connection, not merely a successful login. [O1, O3] | Callback routes either directly store an account or move to an entity picker, useful for Facebook Pages. X uses OAuth 1.0a; Meta exchanges for longer-lived access. [M3, M4] |
| Token storage | Inspected integration repository writes access/refresh token fields directly; no at-rest transform was visible in that path or Prisma service. This is a bounded source observation, not a full security audit. [P4] | App secrets and grants are encrypted. Crypto service has authenticated encryption and a versioned keyring envelope; secret update logic preserves existing values when omitted. [O2, O4] | Account tokens and service configuration use an encrypted array cast through Laravel Crypt; tokens are hidden from normal model serialization. [M1, M5] |
| Refresh and reconnect | Refresh workflow can update account identity and flags failures as needing reconnection; retained refresh tokens are not overwritten with empty values. [P4, P5] | Refresh leases, token versions and revoked-grant predicates prevent concurrent workers from overwriting rotated credentials. Transient failures are treated separately from persistent grant failure. [O5] | Reauthorization upserts by provider/account identity. Unauthorized account updates ask for reauthentication. Do not infer a universal refresh scheduler from these inspected Lite paths. [M6, M7] |
| Disconnect | Marking a channel disconnected is distinct from deleting a channel; controller deletion also deals with related posts. Revocation at the provider was not established by the inspected methods. [P2, P4] | Documentation explains reconnection and secret rotation. Provider-side revocation on disconnect was not established in this bounded review. | Account deletion calls provider revocation when the adapter exposes it, then deletes the local account. [M6] |
| Useful failure guidance | X maps provider errors to actionable reconnect, suspension, body or retry messages. [P3] | Troubleshooting separates missing app configuration, callback mismatch, expired flow, missing destination, test-user limits, media access, and reconnect. OAuth diagnostics allowlist codes instead of echoing raw secret-bearing error text. [O6, O7] | Clear initial configuration alert; callback failures sometimes return a generic retry message. [M2, M3] |

## Design recommendation for beweft

Use two clearly named steps: **Set up provider app** (owner/server configuration) and **Connect channel** (consent and destination selection). Show callback URL with a copy button, exact requested scopes, required account type, provider approval/cost notes, and which settings come from environment. Never show saved secrets again.

Keep provider capability and account health separate. An OAuth success proves identity/consent, not every publishing format, analytics permission, or media-hosting requirement. Distinguish missing configuration, manual-only, connected, partial permissions, expiring, reconnect required, paused and disconnected. Explain the next useful action beside the failure.

Use random single-use expiring state bound to owner/session, workspace, provider and intended reconnect target; PKCE where supported. Encrypt access tokens, refresh tokens and app secrets with a dedicated server key. Serialize token refresh and preserve a previous refresh token if the provider omits a replacement. Reconnection must verify destination identity before replacing credentials. Disconnect must stop queued deliveries, clear local credentials, retain history, and report whether remote revocation succeeded or still requires the provider's app settings.

Persist publication intent and provider receipts before marking success. Keep an ambiguous request in a reconciliation state; never treat a timeout as permission to post again. The source review supports these patterns, but beweft must verify its own behavior independently.

## Source map

All code links are pinned to the inspected revision.

- P1: [Postiz environment example](https://github.com/gitroomhq/postiz-app/blob/64f00fd6d0e9dfd55d202ff83983e7576e08bc2e/.env.example).
- P2: [Integration controller](https://github.com/gitroomhq/postiz-app/blob/64f00fd6d0e9dfd55d202ff83983e7576e08bc2e/apps/backend/src/api/routes/integrations.controller.ts).
- P3: [X provider](https://github.com/gitroomhq/postiz-app/blob/64f00fd6d0e9dfd55d202ff83983e7576e08bc2e/libraries/nestjs-libraries/src/integrations/social/x.provider.ts).
- P4: [Integration repository](https://github.com/gitroomhq/postiz-app/blob/64f00fd6d0e9dfd55d202ff83983e7576e08bc2e/libraries/nestjs-libraries/src/database/prisma/integrations/integration.repository.ts), [Prisma service](https://github.com/gitroomhq/postiz-app/blob/64f00fd6d0e9dfd55d202ff83983e7576e08bc2e/libraries/nestjs-libraries/src/database/prisma/prisma.service.ts).
- P5: [Refresh integration service](https://github.com/gitroomhq/postiz-app/blob/64f00fd6d0e9dfd55d202ff83983e7576e08bc2e/libraries/nestjs-libraries/src/integrations/refresh.integration.service.ts).
- O1: [Self-hosted integration guide](https://github.com/getopenpost/openpost/blob/03ba67e433f5a7e874909f406854453df6120765/apps/docs/content/docs/self-hosting/integrations/index.mdx).
- O2: [Provider app service](https://github.com/getopenpost/openpost/blob/03ba67e433f5a7e874909f406854453df6120765/apps/server/internal/services/providerapps/service.go).
- O3: [OAuth state store](https://github.com/getopenpost/openpost/blob/03ba67e433f5a7e874909f406854453df6120765/apps/server/internal/services/oauthstate/store.go).
- O4: [Encryption service](https://github.com/getopenpost/openpost/blob/03ba67e433f5a7e874909f406854453df6120765/apps/server/internal/services/crypto/encrypt.go).
- O5: [Token manager](https://github.com/getopenpost/openpost/blob/03ba67e433f5a7e874909f406854453df6120765/apps/server/internal/services/tokenmanager/manager.go).
- O6: [Connection troubleshooting](https://github.com/getopenpost/openpost/blob/03ba67e433f5a7e874909f406854453df6120765/apps/docs/content/docs/self-hosting/integrations/troubleshooting.mdx).
- O7: [OAuth feedback](https://github.com/getopenpost/openpost/blob/03ba67e433f5a7e874909f406854453df6120765/apps/server/internal/api/handlers/oauth_feedback.go).
- M1: [Service model](https://github.com/inovector/mixpost/blob/df57648b866310446703f5294350552b62735df5/src/Models/Service.php).
- M2: [Unconfigured service alert](https://github.com/inovector/mixpost/blob/df57648b866310446703f5294350552b62735df5/resources/js/Components/Service/AlertUnconfiguredService.vue).
- M3: [OAuth callback controller](https://github.com/inovector/mixpost/blob/df57648b866310446703f5294350552b62735df5/src/Http/Controllers/CallbackSocialProviderController.php).
- M4: [X OAuth](https://github.com/inovector/mixpost/blob/df57648b866310446703f5294350552b62735df5/src/SocialProviders/Twitter/Concerns/ManagesOAuth.php), [Meta OAuth](https://github.com/inovector/mixpost/blob/df57648b866310446703f5294350552b62735df5/src/SocialProviders/Meta/Concerns/MetaOauth.php).
- M5: [Account model](https://github.com/inovector/mixpost/blob/df57648b866310446703f5294350552b62735df5/src/Models/Account.php), [encryption cast](https://github.com/inovector/mixpost/blob/df57648b866310446703f5294350552b62735df5/src/Casts/EncryptArrayObject.php).
- M6: [Accounts controller](https://github.com/inovector/mixpost/blob/df57648b866310446703f5294350552b62735df5/src/Http/Controllers/AccountsController.php).
- M7: [Account upsert](https://github.com/inovector/mixpost/blob/df57648b866310446703f5294350552b62735df5/src/Actions/UpdateOrCreateAccount.php).
