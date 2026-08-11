CREATE TABLE quotes (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(discord_id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (length(trim(title)) > 0),
    description TEXT NOT NULL CHECK (length(trim(description)) > 0),
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL CHECK (updated_at >= created_at)
);

CREATE TABLE quote_sections (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    quote_id INTEGER NOT NULL REFERENCES quotes(id) ON DELETE CASCADE,
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

CREATE INDEX quotes_user_updated ON quotes(user_id, updated_at DESC);
CREATE INDEX quote_sections_quote_position ON quote_sections(quote_id, position);
