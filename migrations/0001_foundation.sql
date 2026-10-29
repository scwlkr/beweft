CREATE TABLE owners (id uuid PRIMARY KEY, email text NOT NULL UNIQUE, password_hash text NOT NULL, created_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE sessions (token_hash text PRIMARY KEY, owner_id uuid NOT NULL REFERENCES owners(id) ON DELETE CASCADE, expires_at timestamptz NOT NULL);
CREATE TABLE api_tokens (id uuid PRIMARY KEY, owner_id uuid NOT NULL REFERENCES owners(id) ON DELETE CASCADE, name text NOT NULL, token_hash text NOT NULL UNIQUE, scopes jsonb NOT NULL, brand_ids jsonb NOT NULL DEFAULT '[]', created_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE brands (id uuid PRIMARY KEY, name text NOT NULL, color text NOT NULL DEFAULT '#8b5cf6', created_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE channels (id uuid PRIMARY KEY, brand_id uuid NOT NULL REFERENCES brands(id), name text NOT NULL, provider text NOT NULL, timezone text NOT NULL DEFAULT 'UTC', paused boolean NOT NULL DEFAULT false, capabilities jsonb NOT NULL DEFAULT '{}', encrypted_credentials text, created_at timestamptz NOT NULL DEFAULT now());
CREATE INDEX channels_brand ON channels(brand_id);
CREATE TABLE ideas (id uuid PRIMARY KEY, brand_id uuid NOT NULL REFERENCES brands(id), title text NOT NULL, text text NOT NULL DEFAULT '', color text NOT NULL DEFAULT 'lavender', created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE media (id uuid PRIMARY KEY, brand_id uuid NOT NULL REFERENCES brands(id), name text NOT NULL, mime text NOT NULL, size bigint NOT NULL CHECK(size > 0), storage_key text NOT NULL UNIQUE, created_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE posts (id uuid PRIMARY KEY, brand_id uuid NOT NULL REFERENCES brands(id), title text NOT NULL DEFAULT '', text text NOT NULL DEFAULT '', media_ids jsonb NOT NULL DEFAULT '[]', variants jsonb NOT NULL DEFAULT '[]', options jsonb NOT NULL DEFAULT '{}', version integer NOT NULL DEFAULT 1, created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE schedule_rules (id uuid PRIMARY KEY, channel_id uuid NOT NULL REFERENCES channels(id), days jsonb NOT NULL, local_time text NOT NULL, timezone text NOT NULL, created_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE publications (id uuid PRIMARY KEY, post_id uuid NOT NULL REFERENCES posts(id), brand_id uuid NOT NULL REFERENCES brands(id), channel_id uuid NOT NULL REFERENCES channels(id), post_version integer NOT NULL, scheduled_at timestamptz NOT NULL, state text NOT NULL CHECK(state IN ('queued','sending','manual_required','published','fixture_complete','failed','uncertain','canceled')), payload jsonb NOT NULL, external_id text, external_url text, last_error text, lease_until timestamptz, created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now());
CREATE UNIQUE INDEX publication_slot ON publications(channel_id, scheduled_at) WHERE state IN ('queued','sending','manual_required','uncertain');
CREATE UNIQUE INDEX publication_revision_channel ON publications(post_id, post_version, channel_id) WHERE state <> 'canceled';
CREATE INDEX publications_due ON publications(scheduled_at) WHERE state = 'queued';
CREATE INDEX publications_brand ON publications(brand_id);
CREATE TABLE publication_attempts (id uuid PRIMARY KEY, publication_id uuid NOT NULL REFERENCES publications(id), state text NOT NULL, provider text NOT NULL, started_at timestamptz NOT NULL DEFAULT now(), finished_at timestamptz, receipt jsonb, error text);
CREATE INDEX publication_attempt_history ON publication_attempts(publication_id, started_at);
CREATE TABLE worker_heartbeats (id text PRIMARY KEY, last_seen timestamptz NOT NULL);
CREATE TABLE audit_events (id uuid PRIMARY KEY, owner_id uuid REFERENCES owners(id), action text NOT NULL, subject_id uuid, detail jsonb NOT NULL DEFAULT '{}', created_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE auth_limits (key text PRIMARY KEY, attempts integer NOT NULL, window_started timestamptz NOT NULL DEFAULT now());
CREATE TABLE media_references (media_id uuid NOT NULL REFERENCES media(id), post_id uuid REFERENCES posts(id) ON DELETE CASCADE, publication_id uuid REFERENCES publications(id) ON DELETE CASCADE, CHECK ((post_id IS NULL) <> (publication_id IS NULL)));
CREATE INDEX media_references_media ON media_references(media_id);
CREATE FUNCTION maintain_post_media() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 DELETE FROM media_references WHERE post_id=NEW.id;
 INSERT INTO media_references(media_id,post_id)
 SELECT DISTINCT m::uuid,NEW.id FROM (
  SELECT jsonb_array_elements_text(NEW.media_ids) AS m
  UNION SELECT jsonb_array_elements_text(v->'media_ids') FROM jsonb_array_elements(NEW.variants) v WHERE v ? 'media_ids'
 ) refs;
 RETURN NEW;
END $$;
CREATE TRIGGER post_media_reference AFTER INSERT OR UPDATE OF media_ids,variants ON posts FOR EACH ROW EXECUTE FUNCTION maintain_post_media();
CREATE FUNCTION maintain_publication_media() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 INSERT INTO media_references(media_id,publication_id) SELECT DISTINCT m::uuid,NEW.id FROM jsonb_array_elements_text(NEW.payload->'media_ids') m;
 RETURN NEW;
END $$;
CREATE TRIGGER publication_media_reference AFTER INSERT ON publications FOR EACH ROW EXECUTE FUNCTION maintain_publication_media();
