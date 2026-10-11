-- Invitations. An admin makes one; whoever holds its code makes one account with it.

CREATE TABLE invites (
    id         INTEGER PRIMARY KEY,
    code_hash  TEXT NOT NULL UNIQUE,        -- the code is shown once and not kept
    note       TEXT NOT NULL DEFAULT '',    -- who it is for, in the admin's words
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    used_at    INTEGER,                     -- NULL while it is open
    user_id    INTEGER                      -- the account made with it
);
