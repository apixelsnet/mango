-- Two-factor authentication, app passwords, reading progress shared with KOReader, and a
-- source's own address for a title.

-- An authenticator's secret, as hex, once its holder confirmed a code from it; and the
-- last 30-second step a code was taken for, so that one code opens once.
ALTER TABLE users ADD COLUMN totp TEXT;
ALTER TABLE users ADD COLUMN totp_step INTEGER NOT NULL DEFAULT 0;

-- Codes that stand in for the authenticator, once each. Only their hashes are kept.
CREATE TABLE recovery_codes (
    user_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash TEXT NOT NULL,
    PRIMARY KEY (user_id, code_hash)
) WITHOUT ROWID;

-- Passwords Mango made for one reading app each. Hashes only: SHA-256 of the password,
-- and SHA-256 of its MD5, which is what KOReader sends in its place.
CREATE TABLE app_passwords (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    hash       TEXT NOT NULL UNIQUE,
    md5_hash   TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    used_at    INTEGER
);

CREATE INDEX app_passwords_user ON app_passwords(user_id);

-- KOReader's sync: where each document was left, as the app reported it.
CREATE TABLE sync_progress (
    document   TEXT PRIMARY KEY,
    progress   TEXT NOT NULL,
    percentage REAL NOT NULL,
    device     TEXT NOT NULL,
    device_id  TEXT NOT NULL,
    updated_at INTEGER NOT NULL
) WITHOUT ROWID;

-- Which chapter a file Mango handed out is, by the marks KOReader names documents by.
CREATE TABLE sync_documents (
    document   TEXT PRIMARY KEY,
    chapter_id INTEGER NOT NULL REFERENCES chapters(id) ON DELETE CASCADE
) WITHOUT ROWID;

CREATE INDEX sync_documents_chapter ON sync_documents(chapter_id);

-- The title's page on the source's site, as the source itself makes it.
ALTER TABLE manga ADD COLUMN web_url TEXT;
