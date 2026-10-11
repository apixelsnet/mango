-- The highest version of each extension a store has listed. A store's index is not
-- signed, only its extensions are: without this, whoever serves the index could lead
-- an install back to an older version that still carries a valid signature.

CREATE TABLE seen_versions (
    store_id     INTEGER NOT NULL REFERENCES stores(id) ON DELETE CASCADE,
    pkg          TEXT NOT NULL,
    version_code INTEGER NOT NULL,
    PRIMARY KEY (store_id, pkg)
);

INSERT INTO seen_versions (store_id, pkg, version_code) SELECT store_id, pkg, version_code FROM catalog;
