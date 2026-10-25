-- Web Push: browsers and home-screen apps that asked to hear of new chapters.

-- The account's own key for signing what it sends (VAPID). Made on first use.
CREATE TABLE push_key (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    secret BLOB NOT NULL
);

-- One row a device. The address is the browser's push service's; the two keys are the
-- device's own, for encrypting what goes to it.
CREATE TABLE push_devices (
    endpoint TEXT PRIMARY KEY,
    p256dh BLOB NOT NULL,
    auth BLOB NOT NULL,
    -- The page's address when it subscribed: who the push service is told sends.
    origin TEXT,
    added_at INTEGER NOT NULL
);
