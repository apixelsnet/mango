-- Trackers: a link remembers why its progress could not be sent.

-- Set while Mango's row is ahead of the tracker's entry; the next read sends it again.
ALTER TABLE tracks ADD COLUMN error TEXT;
