-- A provider revocation can invalidate a grant created by a separate OAuth flow.
-- Old in-flight flows have no generation and must be authorized again.
ALTER TABLE provider_apps ADD COLUMN oauth_generation bigint NOT NULL DEFAULT 0;
ALTER TABLE provider_apps ADD COLUMN revocation_id uuid;
ALTER TABLE connection_flows ADD COLUMN oauth_generation bigint;
