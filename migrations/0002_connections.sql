CREATE TABLE provider_apps (
 id uuid PRIMARY KEY,
 provider text NOT NULL UNIQUE,
 client_id text NOT NULL,
 instance_url text,
 encrypted_config text NOT NULL,
 updated_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE channels ADD COLUMN connection_identity text;
ALTER TABLE channels ADD COLUMN connection_status text NOT NULL DEFAULT 'manual';
ALTER TABLE channels ADD COLUMN connection_error text;
ALTER TABLE channels ADD COLUMN connected_at timestamptz;
CREATE UNIQUE INDEX connected_destination ON channels(brand_id,provider,connection_identity) WHERE connection_identity IS NOT NULL;
CREATE TABLE connection_flows (
 id uuid PRIMARY KEY,
 provider text NOT NULL,
 owner_id uuid NOT NULL REFERENCES owners(id),
 brand_id uuid NOT NULL REFERENCES brands(id),
 session_hash text NOT NULL,
 state_hash text NOT NULL UNIQUE,
 correlation_hash text NOT NULL,
 encrypted_verifier text NOT NULL,
 reconnect_channel_id uuid REFERENCES channels(id),
 expires_at timestamptz NOT NULL,
 consumed_at timestamptz,
 encrypted_candidates text,
 completed_at timestamptz,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE provider_delivery_checkpoints (
 publication_id uuid PRIMARY KEY REFERENCES publications(id),
 encrypted_checkpoint text NOT NULL,
 updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE provider_media_tokens (
 token_hash text PRIMARY KEY,
 media_id uuid NOT NULL REFERENCES media(id) ON DELETE CASCADE,
 expires_at timestamptz NOT NULL
);
CREATE TABLE provider_metrics (
 publication_id uuid NOT NULL REFERENCES publications(id),
 observed_at timestamptz NOT NULL DEFAULT now(),
 metrics jsonb NOT NULL,
 PRIMARY KEY(publication_id,observed_at)
);
CREATE TABLE provider_inbox (
 id uuid PRIMARY KEY,
 channel_id uuid NOT NULL REFERENCES channels(id),
 provider_item_id text NOT NULL,
 item jsonb NOT NULL,
 read_at timestamptz,
 synced_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(channel_id,provider_item_id)
);
CREATE TABLE provider_syncs (
 channel_id uuid PRIMARY KEY REFERENCES channels(id),
 last_attempt_at timestamptz NOT NULL DEFAULT now(),
 last_success_at timestamptz,
 last_error text,
 coverage text NOT NULL DEFAULT 'recent_page'
);
CREATE TABLE provider_replies (
 id uuid PRIMARY KEY,
 inbox_id uuid NOT NULL REFERENCES provider_inbox(id),
 owner_id uuid NOT NULL REFERENCES owners(id),
 text text NOT NULL,
 state text NOT NULL CHECK(state IN ('sending','published','failed','uncertain')),
 receipt jsonb,
 error text,
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now()
);
