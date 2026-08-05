PRAGMA foreign_keys = ON;

CREATE TABLE operation_batches (
  id TEXT PRIMARY KEY NOT NULL,
  task_id TEXT NOT NULL,
  task_version INTEGER NOT NULL CHECK (task_version > 0),
  status TEXT NOT NULL CHECK (status IN ('queued','running','succeeded','partial','failed','cancelled')),
  created_at INTEGER NOT NULL,
  finished_at INTEGER
);

CREATE TABLE operation_batch_items (
  batch_id TEXT NOT NULL REFERENCES operation_batches(id) ON DELETE CASCADE,
  server_id TEXT NOT NULL REFERENCES servers(id) ON DELETE RESTRICT,
  operation_run_id TEXT REFERENCES operation_runs(id) ON DELETE SET NULL,
  status TEXT NOT NULL CHECK (status IN ('queued','running','succeeded','failed','cancelled')),
  error_message TEXT CHECK (error_message IS NULL OR length(CAST(error_message AS BLOB)) <= 8192),
  PRIMARY KEY (batch_id, server_id)
);

CREATE INDEX idx_operation_batches_created ON operation_batches(created_at DESC);
CREATE INDEX idx_operation_batch_items_server ON operation_batch_items(server_id,status);
