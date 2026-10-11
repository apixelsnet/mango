-- What a Mihon or Suwayomi backup carries that Mango had no place for yet.

ALTER TABLE chapters ADD COLUMN bookmark INTEGER NOT NULL DEFAULT 0;

-- Tracker links, kept as the backup gives them until trackers are built.
CREATE TABLE tracks (
    manga_id          INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    tracker           INTEGER NOT NULL,           -- the tracker's id in Mihon: 1 MyAnimeList, 2 AniList, 3 Kitsu, ...
    library_id        INTEGER NOT NULL DEFAULT 0, -- the entry's id on the tracker
    media_id          INTEGER NOT NULL DEFAULT 0, -- the title's id on the tracker
    url               TEXT NOT NULL DEFAULT '',
    title             TEXT NOT NULL DEFAULT '',
    last_chapter_read REAL NOT NULL DEFAULT 0,
    total_chapters    INTEGER NOT NULL DEFAULT 0,
    score             REAL NOT NULL DEFAULT 0,
    status            INTEGER NOT NULL DEFAULT 0,
    started_at        INTEGER NOT NULL DEFAULT 0, -- epoch milliseconds, as trackers and backups use
    finished_at       INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (manga_id, tracker)
) WITHOUT ROWID;

-- Finds the extension that provides a source a backup refers to.
CREATE INDEX catalog_sources_source ON catalog_sources(source_id);
