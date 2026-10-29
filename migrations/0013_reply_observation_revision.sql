-- A timestamp can collide or move backwards. Every reply mutation receives a
-- durable, strictly increasing revision for owner recovery decisions.
ALTER TABLE provider_replies ADD COLUMN revision bigint NOT NULL DEFAULT 1;

CREATE FUNCTION advance_provider_reply_revision() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  NEW.revision := OLD.revision + 1;
  RETURN NEW;
END
$$;

CREATE TRIGGER provider_reply_revision BEFORE UPDATE ON provider_replies
FOR EACH ROW EXECUTE FUNCTION advance_provider_reply_revision();
