-- Expiry follows the actual claim time, never transaction start before lock waits.
ALTER TABLE provider_replies ADD COLUMN delivery_deadline timestamptz;
UPDATE provider_replies SET delivery_deadline=created_at+interval '2 minutes';
ALTER TABLE provider_replies ALTER COLUMN delivery_deadline SET NOT NULL;
ALTER TABLE provider_replies ALTER COLUMN delivery_deadline SET DEFAULT (clock_timestamp()+interval '90 seconds');
ALTER TABLE provider_replies ALTER COLUMN created_at SET DEFAULT clock_timestamp();
