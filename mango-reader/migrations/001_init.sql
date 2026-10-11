-- Extension stores, their catalogs, and what is installed.

CREATE TABLE stores (
    id          INTEGER PRIMARY KEY,
    url         TEXT NOT NULL,              -- as the owner typed it
    index_url   TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    badge       TEXT NOT NULL DEFAULT '',
    website     TEXT,
    signing_key TEXT NOT NULL,              -- pinned when the store was added; sha256 of the certificate, hex
    offered_key TEXT,                       -- set while the store publishes a key other than the pinned one
    etag        TEXT,
    checked_at  INTEGER,                    -- epoch seconds, like every time below
    updated_at  INTEGER,
    error       TEXT,
    added_at    INTEGER NOT NULL
);

CREATE TABLE catalog (
    store_id        INTEGER NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
    pkg             TEXT NOT NULL,
    name            TEXT NOT NULL,
    version_name    TEXT NOT NULL,
    version_code    INTEGER NOT NULL,
    lib             TEXT NOT NULL,
    content_warning INTEGER NOT NULL,       -- 0 or 1 safe, 2 mixed, 3 NSFW
    jar_url         TEXT,
    apk_url         TEXT,
    icon_url        TEXT,
    jar_sha256      TEXT,
    langs           TEXT NOT NULL,          -- ",en,es," so one LIKE finds a language
    source_count    INTEGER NOT NULL,
    search          TEXT NOT NULL,          -- lower-case name, package and source names
    PRIMARY KEY (store_id, pkg)
) WITHOUT ROWID;

CREATE TABLE catalog_sources (
    store_id  INTEGER NOT NULL,
    pkg       TEXT NOT NULL,
    source_id INTEGER NOT NULL,
    name      TEXT NOT NULL,
    lang      TEXT NOT NULL,
    home_url  TEXT,
    PRIMARY KEY (store_id, pkg, source_id),
    FOREIGN KEY (store_id, pkg) REFERENCES catalog(store_id, pkg) ON DELETE CASCADE
) WITHOUT ROWID;

-- Installed extensions. The host owns the files; this mirror lets the core list
-- them and their sources without starting it.
CREATE TABLE extensions (
    pkg             TEXT PRIMARY KEY,
    store_id        INTEGER REFERENCES stores(id) ON DELETE SET NULL,
    name            TEXT NOT NULL,
    version_name    TEXT NOT NULL,
    version_code    INTEGER NOT NULL,
    lib             TEXT NOT NULL,
    content_warning INTEGER NOT NULL,
    icon_url        TEXT,
    installed_at    INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
) WITHOUT ROWID;

CREATE INDEX extensions_store ON extensions(store_id);

CREATE TABLE sources (
    id              INTEGER PRIMARY KEY,
    pkg             TEXT NOT NULL REFERENCES extensions(pkg) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    lang            TEXT NOT NULL,
    base_url        TEXT,
    supports_latest INTEGER NOT NULL
);

CREATE INDEX sources_pkg ON sources(pkg);
