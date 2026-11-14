-- Chapters a source stopped listing, remembered for a while: a source that leaves some out of
-- one answer and lists them again later gives back the same chapters, with what was read of
-- them, and they are not news. `at` is the chapter's number to the hundredth ("n1250") or,
-- with no number, its name ("s..."); `grp` its scanlation group, lowercased.
CREATE TABLE chapters_left (
    manga_id       INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    at             TEXT NOT NULL,
    grp            TEXT NOT NULL,
    read           INTEGER NOT NULL,
    last_page_read INTEGER NOT NULL,
    bookmark       INTEGER NOT NULL,
    read_at        INTEGER,
    fetched_at     INTEGER NOT NULL,
    left_at        INTEGER NOT NULL
);

CREATE INDEX chapters_left_at ON chapters_left(manga_id, at, grp);

-- When a title was last shown while browsing or opened, in epoch seconds. A title outside
-- the library that holds nothing of the reader's and was not seen for months is let go.
ALTER TABLE manga ADD COLUMN seen_at INTEGER;
-- An app password that is an API token: a script sends it as `Authorization: Bearer` and
-- may call /api as the account. Other app passwords open only the reading-app routes.
ALTER TABLE app_passwords ADD COLUMN api INTEGER NOT NULL DEFAULT 0;

-- Titles stored before this count as seen now, so none goes at once.
UPDATE manga SET seen_at = CAST(strftime('%s', 'now') AS INTEGER);
