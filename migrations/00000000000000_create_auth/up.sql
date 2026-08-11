PRAGMA foreign_keys = ON;

CREATE TABLE users (
    discord_id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL,
    avatar_hash TEXT,
    created_at BIGINT NOT NULL
);

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(discord_id) ON DELETE CASCADE,
    created_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL
);

CREATE INDEX sessions_expires_at ON sessions(expires_at);
