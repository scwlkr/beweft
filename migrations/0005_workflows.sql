ALTER TABLE brands ADD COLUMN archived_at timestamptz;
ALTER TABLE media ADD COLUMN alt_text text NOT NULL DEFAULT '';
ALTER TABLE channels ADD COLUMN target_platform text;
UPDATE channels SET target_platform='generic' WHERE provider='manual';
ALTER TABLE channels ADD CONSTRAINT manual_target CHECK (target_platform IS NULL OR (provider='manual' AND target_platform IN ('generic','x','youtube','facebook','instagram','threads','tiktok','bluesky','mastodon','linkedin','pinterest')));
CREATE TABLE idea_groups (id uuid PRIMARY KEY,brand_id uuid NOT NULL REFERENCES brands(id),name text NOT NULL,color text NOT NULL DEFAULT 'lavender',position integer NOT NULL DEFAULT 0,created_at timestamptz NOT NULL DEFAULT now());
ALTER TABLE ideas ADD COLUMN status text NOT NULL DEFAULT 'inbox' CHECK (status IN ('inbox','in_progress','ready','archived'));
ALTER TABLE ideas ADD COLUMN group_id uuid REFERENCES idea_groups(id) ON DELETE SET NULL;
ALTER TABLE ideas ADD COLUMN tags jsonb NOT NULL DEFAULT '[]';
CREATE TABLE templates (id uuid PRIMARY KEY,brand_id uuid NOT NULL REFERENCES brands(id),name text NOT NULL,title text NOT NULL DEFAULT '',text text NOT NULL DEFAULT '',media_ids jsonb NOT NULL DEFAULT '[]',variants jsonb NOT NULL DEFAULT '[]',options jsonb NOT NULL DEFAULT '{}',version integer NOT NULL DEFAULT 1,created_at timestamptz NOT NULL DEFAULT now(),updated_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE template_media_references (media_id uuid NOT NULL REFERENCES media(id),template_id uuid NOT NULL REFERENCES templates(id) ON DELETE CASCADE,PRIMARY KEY(media_id,template_id));
CREATE FUNCTION maintain_template_media() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 DELETE FROM template_media_references WHERE template_id=NEW.id;
 INSERT INTO template_media_references(media_id,template_id)
 SELECT DISTINCT m::uuid,NEW.id FROM (SELECT jsonb_array_elements_text(NEW.media_ids) AS m UNION SELECT jsonb_array_elements_text(v->'media_ids') FROM jsonb_array_elements(NEW.variants) v WHERE v ? 'media_ids') refs;
 RETURN NEW;
END $$;
CREATE TRIGGER template_media_reference AFTER INSERT OR UPDATE OF media_ids,variants ON templates FOR EACH ROW EXECUTE FUNCTION maintain_template_media();
ALTER TABLE sessions ADD COLUMN id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE;
ALTER TABLE sessions ADD COLUMN created_at timestamptz NOT NULL DEFAULT now();
