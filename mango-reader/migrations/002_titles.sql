-- Titles, chapters, per-source choices and settings.

-- Kept apart from `sources`, whose rows are rewritten on every extension update.
CREATE TABLE source_prefs (
    source_id INTEGER PRIMARY KEY,
    pinned    INTEGER NOT NULL DEFAULT 0,
    hidden    INTEGER NOT NULL DEFAULT 0
);

-- Every title a source has shown, in the library or not.
CREATE TABLE manga (
    id              INTEGER PRIMARY KEY,
    source_id       INTEGER NOT NULL,
    url             TEXT NOT NULL,              -- the source's own key for the title
    title           TEXT NOT NULL,
    artist          TEXT,
    author          TEXT,
    description     TEXT,
    genre           TEXT,                       -- comma separated, as the source gives it
    status          INTEGER NOT NULL DEFAULT 0, -- 0 unknown, 1 ongoing, 2 completed, 3 licensed, 4 finished, 5 cancelled, 6 hiatus
    thumbnail_url   TEXT,
    update_strategy TEXT NOT NULL DEFAULT 'ALWAYS_UPDATE',
    initialized     INTEGER NOT NULL DEFAULT 0, -- details were fetched at least once
    memo            TEXT NOT NULL DEFAULT '{}', -- the extension's own state, sent back unchanged
    in_library      INTEGER NOT NULL DEFAULT 0,
    added_at        INTEGER,                    -- epoch seconds; when it joined the library
    details_at      INTEGER,
    chapters_at     INTEGER,
    reader_mode     TEXT,                       -- ltr, rtl, vertical, webtoon; NULL follows the default
    reader_fit      TEXT,                       -- width, height, original
    auto_download   INTEGER NOT NULL DEFAULT 0,
    UNIQUE (source_id, url)
);

CREATE INDEX manga_library ON manga(in_library) WHERE in_library = 1;

CREATE TABLE chapters (
    id             INTEGER PRIMARY KEY,
    manga_id       INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    url            TEXT NOT NULL,
    name           TEXT NOT NULL,
    date_upload    INTEGER NOT NULL DEFAULT 0,  -- epoch milliseconds from the source, 0 when unknown
    chapter_number REAL NOT NULL DEFAULT -1,
    scanlator      TEXT,
    memo           TEXT NOT NULL DEFAULT '{}',
    source_order   INTEGER NOT NULL,            -- place in the source's list; 0 is listed first, usually the newest
    fetched_at     INTEGER NOT NULL,            -- epoch seconds; when Mango first saw it
    read           INTEGER NOT NULL DEFAULT 0,
    last_page_read INTEGER NOT NULL DEFAULT 0,
    read_at        INTEGER,
    page_count     INTEGER,                     -- known once the chapter was opened
    download_path  TEXT,                        -- the CBZ, relative to the downloads folder
    UNIQUE (manga_id, url)
);

CREATE INDEX chapters_order ON chapters(manga_id, source_order);
CREATE INDEX chapters_fetched ON chapters(fetched_at);

-- Preferences. Values are JSON.
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;
