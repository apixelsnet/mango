-- The download queue, and what went wrong in the last library update.

-- A chapter waits here until it is saved; then the row goes and
-- `chapters.download_path` says where the file is.
CREATE TABLE downloads (
    chapter_id INTEGER PRIMARY KEY REFERENCES chapters(id) ON DELETE CASCADE,
    position   INTEGER NOT NULL,
    state      TEXT NOT NULL DEFAULT 'queued',  -- queued, downloading, failed
    attempts   INTEGER NOT NULL DEFAULT 0,
    retry_at   INTEGER,                         -- epoch seconds; not tried again before then
    error      TEXT,
    added_at   INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX downloads_position ON downloads(position);

-- One row per title whose last update failed. A later success removes it.
CREATE TABLE update_errors (
    manga_id INTEGER PRIMARY KEY REFERENCES manga(id) ON DELETE CASCADE,
    error    TEXT NOT NULL,
    at       INTEGER NOT NULL
) WITHOUT ROWID;
