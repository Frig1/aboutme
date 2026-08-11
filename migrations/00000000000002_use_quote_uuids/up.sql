DROP INDEX quote_sections_quote_position;
DROP INDEX quotes_user_updated;

ALTER TABLE quote_sections RENAME TO quote_sections_integer;
ALTER TABLE quotes RENAME TO quotes_integer;

CREATE TABLE quotes (
    id TEXT PRIMARY KEY NOT NULL DEFAULT (
        lower(
            hex(randomblob(4)) || '-' ||
            hex(randomblob(2)) || '-4' ||
            substr(hex(randomblob(2)), 2) || '-' ||
            substr('89ab', (random() & 3) + 1, 1) ||
            substr(hex(randomblob(2)), 2) || '-' ||
            hex(randomblob(6))
        )
    ) CHECK (
        length(id) = 36 AND
        substr(id, 9, 1) = '-' AND
        substr(id, 14, 1) = '-' AND
        substr(id, 15, 1) = '4' AND
        substr(id, 19, 1) = '-' AND
        lower(substr(id, 20, 1)) IN ('8', '9', 'a', 'b') AND
        substr(id, 24, 1) = '-' AND
        id NOT GLOB '*[^0-9a-fA-F-]*'
    ),
    user_id TEXT NOT NULL REFERENCES users(discord_id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    description TEXT NOT NULL CHECK (length(trim(description)) > 0),
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL CHECK (updated_at >= created_at)
);

CREATE TABLE quote_sections (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    quote_id TEXT NOT NULL REFERENCES quotes(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    description TEXT NOT NULL CHECK (length(trim(description)) > 0),
    estimate_min_minutes INTEGER NOT NULL CHECK (estimate_min_minutes > 0),
    estimate_max_minutes INTEGER CHECK (
        estimate_max_minutes IS NULL OR estimate_max_minutes >= estimate_min_minutes
    ),
    price_cents BIGINT NOT NULL CHECK (price_cents >= 0),
    UNIQUE (quote_id, position)
);

CREATE TABLE quote_id_map (
    integer_id INTEGER PRIMARY KEY NOT NULL,
    uuid TEXT NOT NULL UNIQUE
);

INSERT INTO quote_id_map
SELECT
    id,
    lower(
        hex(randomblob(4)) || '-' ||
        hex(randomblob(2)) || '-4' ||
        substr(hex(randomblob(2)), 2) || '-' ||
        substr('89ab', (random() & 3) + 1, 1) ||
        substr(hex(randomblob(2)), 2) || '-' ||
        hex(randomblob(6))
    )
FROM quotes_integer;

INSERT INTO quotes (id, user_id, title, description, created_at, updated_at)
SELECT map.uuid, old.user_id, old.title, old.description, old.created_at, old.updated_at
FROM quotes_integer AS old
JOIN quote_id_map AS map ON map.integer_id = old.id;

INSERT INTO quote_sections (
    id,
    quote_id,
    position,
    title,
    description,
    estimate_min_minutes,
    estimate_max_minutes,
    price_cents
)
SELECT
    old.id,
    map.uuid,
    old.position,
    old.title,
    old.description,
    old.estimate_min_minutes,
    old.estimate_max_minutes,
    old.price_cents
FROM quote_sections_integer AS old
JOIN quote_id_map AS map ON map.integer_id = old.quote_id;

DROP TABLE quote_sections_integer;
DROP TABLE quotes_integer;
DROP TABLE quote_id_map;

CREATE INDEX quotes_user_updated ON quotes(user_id, updated_at DESC);
CREATE INDEX quote_sections_quote_position ON quote_sections(quote_id, position);
