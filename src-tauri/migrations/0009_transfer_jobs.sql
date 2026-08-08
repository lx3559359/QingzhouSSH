CREATE TABLE transfer_jobs (
  id TEXT PRIMARY KEY NOT NULL,
  execution_id TEXT NULL REFERENCES executions(id) ON DELETE SET NULL,
  server_id TEXT NOT NULL REFERENCES servers(id) ON DELETE RESTRICT,
  direction TEXT NOT NULL CHECK (direction IN ('upload', 'download')),
  source_path TEXT NOT NULL,
  target_path TEXT NOT NULL,
  overwrite INTEGER NOT NULL DEFAULT 0 CHECK (overwrite IN (0, 1)),
  verification TEXT NOT NULL CHECK (verification IN ('balanced', 'strict', 'transport_only')),
  status TEXT NOT NULL CHECK (status IN ('queued', 'connecting', 'transferring', 'verifying', 'finalizing', 'succeeded', 'failed', 'cancelled', 'uncertain')),
  transferred INTEGER NOT NULL DEFAULT 0 CHECK (transferred >= 0),
  total INTEGER NULL CHECK (total IS NULL OR total >= 0),
  percent REAL NULL CHECK (percent IS NULL OR (percent >= 0 AND percent <= 100)),
  bytes_per_second REAL NULL CHECK (bytes_per_second IS NULL OR bytes_per_second >= 0),
  average_bytes_per_second REAL NULL CHECK (average_bytes_per_second IS NULL OR average_bytes_per_second >= 0),
  eta_seconds INTEGER NULL CHECK (eta_seconds IS NULL OR eta_seconds >= 0),
  attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
  max_attempts INTEGER NOT NULL DEFAULT 3 CHECK (max_attempts BETWEEN 1 AND 10),
  cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK (cancel_requested IN (0, 1)),
  retryable INTEGER NOT NULL DEFAULT 0 CHECK (retryable IN (0, 1)),
  error_category TEXT NULL,
  error_message TEXT NULL,
  sha256 TEXT NULL,
  location TEXT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  started_at INTEGER NULL,
  finished_at INTEGER NULL
);

CREATE INDEX idx_transfer_jobs_queue ON transfer_jobs(status, created_at, id);
CREATE INDEX idx_transfer_jobs_server ON transfer_jobs(server_id, created_at DESC);
CREATE INDEX idx_transfer_jobs_execution ON transfer_jobs(execution_id);
