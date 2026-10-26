-- The reading stats walk every chapter read, by when it was read: by day for the
-- streak, by month for the chart. Only read chapters are in the index.

CREATE INDEX chapters_read_at ON chapters(read_at) WHERE read = 1 AND read_at IS NOT NULL;
