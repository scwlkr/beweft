-- A format rejection is terminal for its request key, even if adapter rules
-- or server configuration later change. Existing receipts remain accepted.
-- Rejected requests retain their original post UUID even after the owner
-- deletes an unpublished draft. Accepted publication history retains its
-- separate publications.post_id foreign key.
ALTER TABLE schedule_requests DROP CONSTRAINT schedule_requests_post_id_fkey;
ALTER TABLE schedule_requests
    ADD COLUMN outcome text NOT NULL DEFAULT 'accepted' CHECK (outcome IN ('accepted', 'rejected')),
    ADD COLUMN error_code text,
    ADD COLUMN error_message text,
    ADD CONSTRAINT schedule_request_outcome_error CHECK (
        (outcome = 'accepted' AND error_code IS NULL AND error_message IS NULL)
        OR (outcome = 'rejected' AND error_code IS NOT NULL
            AND error_code IN ('provider_format_invalid', 'schedule_request_retired')
            AND error_message IS NOT NULL AND char_length(error_message) BETWEEN 1 AND 2048)
    );
