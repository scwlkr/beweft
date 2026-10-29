# LinkedIn organization publishing checkpoint

Checked 2026-10-29 against Microsoft's LinkedIn documentation. This is research
and a proposed implementation boundary, not an implemented capability or live
account qualification. Current member publishing must keep working without
requesting organization access.

## Provider contract

The [Posts API](https://learn.microsoft.com/en-us/linkedin/marketing/community-management/shares/posts-api?view=li-lms-2026-10)
uses an organization URN as author for organization posts and requires
`w_organization_social`; its supported-role list includes `ADMINISTRATOR`,
`DIRECT_SPONSORED_CONTENT_POSTER`, and `CONTENT_ADMIN`. Member publishing uses
`w_member_social`. Requests require the version and Rest.li protocol headers;
a create receipt is HTTP 201 plus `x-restli-id`. Existing image/video upload
stages can be extended with the selected organization owner, without treating
an arbitrary caller-supplied URN as authorization. Organization read permission
is distinct from publishing permission.

The [organization ACL reference](https://learn.microsoft.com/en-us/linkedin/marketing/community-management/organizations/organization-access-control-by-role?view=li-lms-2026-10)
documents `GET /rest/organizationAcls?q=roleAssignee`, approved versus
requested/rejected/revoked role states, and paginated results. Its examples use
both `organization` and `organizationTarget`. Discovery needs strict URN parsing
and consistent handling of those documented forms; conflicting fields must not
select a different destination. The reference spells the content-management
role `CONTENT_ADMINISTRATOR`, unlike the Posts API's `CONTENT_ADMIN`. That
inconsistency needs resolving or an explicitly narrower supported-role policy.
Organization names come from the linked [organization lookup API](https://learn.microsoft.com/en-us/linkedin/marketing/community-management/organizations/organization-lookup-api?view=li-lms-2026-10),
not from a fabricated destination label.

[Increasing Access](https://learn.microsoft.com/en-us/linkedin/marketing/increasing-access?view=li-lms-2026-06)
separates approved API products and their scope sets. The
[Community Management migration/access guide](https://learn.microsoft.com/en-us/linkedin/marketing/community-management/community-management-api-migration-guide?view=li-lms-2024-05)
requires application review and documents development-tier request restrictions.
The ACL reference lists read-only `r_organization_admin` as well as
`rw_organization_admin`; the Community Management scope list includes the latter.
Do not assume a self-service Share app has either scope or silently request
broader administration access to make discovery work.

## Proposed next milestone

Make organization access an explicit provider-app opt-in with the exact approved
scope requirements shown before authorization. Default member configuration and
existing member grants remain valid. Discover the member plus fully paginated,
approved, supported organization roles; retain authoritative candidates behind
the existing expiring encrypted selection flow. Denied optional organization
access must be shown clearly rather than represented as an empty successful list.

A selected organization is a distinct stable channel identity. Reconnect must
match that identity; never move a member channel or its queue to an organization.
Publishing reuses the durable member media/public-write recovery boundaries with
organization ownership. Fixture checks should cover unchanged default member
scopes, opt-in discovery/denial/pagination, stale reconnects and matching-owner
image/video/post requests. Broader reads and replies need their own actual scopes
and capability reporting. No live account, paid request, or public post was used.
