-- A provider revocation can invalidate a grant created by a separate OAuth flow.
-- Old in-flight flows have no generation and must be authorized again.
CREATE TABLE provider_oauth_fences (
 provider text PRIMARY KEY,
 oauth_generation bigint NOT NULL DEFAULT 0,
 revocation_id uuid
);
-- A missing or replaced app configuration must not erase an existing fence.
INSERT INTO provider_oauth_fences(provider)
 SELECT provider FROM provider_apps
 UNION SELECT provider FROM channels WHERE provider NOT IN ('manual','fixture');
ALTER TABLE connection_flows ADD COLUMN oauth_generation bigint;
