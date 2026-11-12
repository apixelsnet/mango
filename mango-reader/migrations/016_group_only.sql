-- Reading a title stays with one scanlation group: on to that group's next chapter, passing
-- over the chapters it has not uploaded, instead of filling them in from other groups.
ALTER TABLE manga ADD COLUMN group_only INTEGER NOT NULL DEFAULT 0;
