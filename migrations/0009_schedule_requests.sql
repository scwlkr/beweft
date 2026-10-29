-- The owner-scoped request key is an immutable record of a committed intent.
-- The ordered links retain the original publication identities after cancellation
-- or replacement; failed transactions create neither the request nor its links.
CREATE TABLE schedule_requests (
    owner_id uuid NOT NULL REFERENCES owners(id),
    request_id uuid NOT NULL,
    post_id uuid NOT NULL REFERENCES posts(id),
    brand_id uuid NOT NULL REFERENCES brands(id),
    post_version integer NOT NULL,
    channel_ids uuid[] NOT NULL CHECK (cardinality(channel_ids) BETWEEN 1 AND 50),
    scheduled_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (owner_id, request_id)
);

CREATE TABLE schedule_request_publications (
    owner_id uuid NOT NULL,
    request_id uuid NOT NULL,
    position smallint NOT NULL CHECK (position BETWEEN 0 AND 49),
    publication_id uuid NOT NULL UNIQUE REFERENCES publications(id),
    PRIMARY KEY (owner_id, request_id, position),
    FOREIGN KEY (owner_id, request_id) REFERENCES schedule_requests(owner_id, request_id)
);
