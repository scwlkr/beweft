-- Existing rows are conservatively treated as exact; their original slot intent was not recorded.
ALTER TABLE publications ADD COLUMN scheduling_mode text NOT NULL DEFAULT 'exact' CHECK (scheduling_mode IN ('queue', 'exact'));
CREATE TABLE media_upload_reservations (id uuid PRIMARY KEY, reserved_bytes bigint NOT NULL CHECK (reserved_bytes > 0), expires_at timestamptz NOT NULL);
