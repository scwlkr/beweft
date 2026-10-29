-- Claim order must not depend on transaction-start timestamps or wall-clock adjustments.
CREATE SEQUENCE provider_reply_history_order;
ALTER TABLE provider_replies ADD COLUMN history_order bigint;
WITH ordered AS (SELECT id,row_number() OVER (ORDER BY created_at,id) AS position FROM provider_replies)
UPDATE provider_replies r SET history_order=o.position FROM ordered o WHERE o.id=r.id;
SELECT setval('provider_reply_history_order',GREATEST(COALESCE(max(history_order),0),1),COALESCE(max(history_order),0)>0) FROM provider_replies;
ALTER TABLE provider_replies ALTER COLUMN history_order SET DEFAULT nextval('provider_reply_history_order');
ALTER TABLE provider_replies ALTER COLUMN history_order SET NOT NULL;
ALTER SEQUENCE provider_reply_history_order OWNED BY provider_replies.history_order;
CREATE UNIQUE INDEX provider_reply_history_position ON provider_replies(history_order);
CREATE INDEX provider_reply_item_history ON provider_replies(inbox_id,history_order DESC);
