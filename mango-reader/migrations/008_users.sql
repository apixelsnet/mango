-- Accounts. Each has a library of its own; the first one runs the server.

CREATE TABLE users (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password   TEXT,                        -- Argon2 hash; NULL until one is set
    admin      INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

-- The password Mango had before it knew about accounts becomes the first account's.
INSERT INTO users (id, name, password, admin, created_at)
SELECT 1, 'owner', value, 1, CAST(strftime('%s', 'now') AS INTEGER) FROM state WHERE key = 'auth.password';

DELETE FROM state WHERE key = 'auth.password';

ALTER TABLE sessions ADD COLUMN user_id INTEGER NOT NULL DEFAULT 1;
