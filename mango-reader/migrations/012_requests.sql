-- What readers of Mihon and its forks ask for most: titles edited by hand, one title
-- read from several sources, a say in which group's upload is read, categories inside
-- categories, folders for sources, and notes on single pages.

-- The detail fields edited by hand, as a JSON list ("title", "cover", ...). A refresh
-- from the source leaves them as they are.
ALTER TABLE manga ADD COLUMN locked TEXT;

-- Scanlation groups in the order they are wanted, as a JSON list. Groups not in it come after.
ALTER TABLE manga ADD COLUMN scanlators TEXT;

-- Show one upload per chapter: the one the order above and the sources' order prefer.
ALTER TABLE manga ADD COLUMN dedupe INTEGER NOT NULL DEFAULT 0;

-- Set on a title that was merged into another: its chapters show and update under that
-- one, and it is not in the library on its own. Merged titles come in this order.
ALTER TABLE manga ADD COLUMN merged_into INTEGER REFERENCES manga(id) ON DELETE SET NULL;
ALTER TABLE manga ADD COLUMN merged_at INTEGER;

CREATE INDEX manga_merged ON manga(merged_into) WHERE merged_into IS NOT NULL;

-- A category may sit inside another. Showing the outer one shows what is inside it too.
ALTER TABLE categories ADD COLUMN parent_id INTEGER REFERENCES categories(id) ON DELETE SET NULL;

-- A folder of the owner's own name for a source on Browse. NULL: none.
ALTER TABLE source_prefs ADD COLUMN folder TEXT;

-- A page kept with a note of its own.
CREATE TABLE page_marks (
    chapter_id INTEGER NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
    page       INTEGER NOT NULL,             -- from 0, as the reader counts
    note       TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    PRIMARY KEY (chapter_id, page)
) WITHOUT ROWID;
