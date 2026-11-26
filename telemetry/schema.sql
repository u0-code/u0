-- The one table the Worker writes: a row a batch, its maps as JSON. A
-- column is null where the batch's level does not send it.
CREATE TABLE IF NOT EXISTS batches (
  at TEXT NOT NULL DEFAULT (datetime('now')),
  country TEXT NOT NULL,
  level INTEGER NOT NULL,
  version TEXT NOT NULL,
  "commit" TEXT NOT NULL,
  channel TEXT NOT NULL,
  os TEXT NOT NULL,
  arch TEXT NOT NULL,
  id TEXT,
  agents TEXT,
  servers INTEGER,
  listening INTEGER,
  sessions TEXT,
  turns INTEGER,
  worktrees INTEGER,
  tasks INTEGER,
  runs INTEGER,
  tools TEXT,
  opened TEXT,
  crashes INTEGER,
  startup_ms INTEGER,
  memory_mb INTEGER,
  errors TEXT,
  models TEXT,
  width INTEGER,
  height INTEGER,
  zoom INTEGER,
  look TEXT,
  language TEXT
);
CREATE INDEX IF NOT EXISTS batches_at ON batches (at);
