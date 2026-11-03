CREATE TABLE start_pages (
  id uuid PRIMARY KEY,
  brand_id uuid NOT NULL REFERENCES brands(id) ON DELETE CASCADE,
  slug text NOT NULL,
  title text NOT NULL DEFAULT '',
  bio text NOT NULL DEFAULT '',
  theme text NOT NULL DEFAULT 'classic',
  avatar_media_id uuid REFERENCES media(id),
  published boolean NOT NULL DEFAULT true,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE(brand_id),
  UNIQUE(slug)
);

CREATE TABLE start_page_links (
  id uuid PRIMARY KEY,
  start_page_id uuid NOT NULL REFERENCES start_pages(id) ON DELETE CASCADE,
  title text NOT NULL,
  url text NOT NULL,
  position integer NOT NULL DEFAULT 0,
  enabled boolean NOT NULL DEFAULT true,
  clicks integer NOT NULL DEFAULT 0,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX start_page_links_page ON start_page_links(start_page_id, position);
CREATE INDEX start_pages_slug ON start_pages(slug) WHERE published = true;
