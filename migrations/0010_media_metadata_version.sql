ALTER TABLE media ADD COLUMN metadata_version integer NOT NULL DEFAULT 1
    CHECK (metadata_version > 0);
