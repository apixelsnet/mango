-- Login, and what the server keeps to itself.

-- Values the settings API never shows: the password hash, the last answer from the release list.
CREATE TABLE state (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

-- Signed-in browsers. Only the hash of a session's token is stored.
CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL,
    seen_at    INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    agent      TEXT NOT NULL DEFAULT ''  -- the browser's name for itself, to tell sessions apart
) WITHOUT ROWID;
