-- Null preserves the behavior of precondition-free legacy requests. A JSON
-- object holds the exact canonical media-version map approved for new intents.
ALTER TABLE schedule_requests ADD COLUMN media_versions jsonb
    CHECK (media_versions IS NULL OR jsonb_typeof(media_versions) = 'object');
