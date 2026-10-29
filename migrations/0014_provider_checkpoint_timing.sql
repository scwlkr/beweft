-- DB-clock processing waits and upload-session expiry survive worker restarts.
-- Existing providers have NULL timing and keep their prior scheduling behavior.
ALTER TABLE provider_delivery_checkpoints
  ADD COLUMN not_before timestamptz,
  ADD COLUMN expires_at timestamptz,
  ADD COLUMN checkpoint_id uuid NOT NULL DEFAULT gen_random_uuid(),
  ADD COLUMN attempt_id uuid;

-- Legacy X image checkpoints have no embedded identity. Preserve a stable,
-- local observation identity so owners can resolve them without resending.
UPDATE provider_delivery_checkpoints cp SET attempt_id = COALESCE(
  (SELECT a.id FROM publication_attempts a
   WHERE a.publication_id = cp.publication_id
     AND COALESCE(a.receipt->>'verification','') <> 'owner_attestation'
   ORDER BY a.started_at DESC, a.id DESC LIMIT 1),
  gen_random_uuid()
);
