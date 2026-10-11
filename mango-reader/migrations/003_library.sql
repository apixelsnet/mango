-- The library's categories, and the reading history.

CREATE TABLE categories (
    id       INTEGER PRIMARY KEY,
    name     TEXT NOT NULL UNIQUE COLLATE NOCASE,
    position INTEGER NOT NULL
);

-- A title can be in several categories, or in none.
CREATE TABLE manga_categories (
    manga_id    INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    category_id INTEGER NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
    PRIMARY KEY (manga_id, category_id)
) WITHOUT ROWID;

CREATE INDEX manga_categories_category ON manga_categories(category_id);

-- One row per chapter that was opened in the reader: when it was last read.
CREATE TABLE history (
    chapter_id INTEGER PRIMARY KEY REFERENCES chapters(id) ON DELETE CASCADE,
    manga_id   INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
    read_at    INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX history_time ON history(read_at);
CREATE INDEX history_manga ON history(manga_id, read_at);
