PRAGMA foreign_keys = ON;

CREATE TABLE workflows (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
  description TEXT NOT NULL CHECK (length(CAST(description AS BLOB)) <= 4096),
  current_version INTEGER NOT NULL CHECK (current_version > 0),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE workflow_versions (
  workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
  version INTEGER NOT NULL CHECK (version > 0),
  definition_json TEXT NOT NULL,
  checksum_sha256 TEXT NOT NULL CHECK (length(checksum_sha256) = 64),
  created_at INTEGER NOT NULL,
  PRIMARY KEY (workflow_id, version)
);

CREATE TABLE workflow_runs (
  id TEXT PRIMARY KEY NOT NULL,
  workflow_id TEXT NOT NULL,
  workflow_version INTEGER NOT NULL CHECK (workflow_version > 0),
  server_id TEXT NOT NULL REFERENCES servers(id) ON DELETE RESTRICT,
  status TEXT NOT NULL CHECK (status IN ('queued','running','paused','succeeded','cancelled','uncertain','rolled_back','rollback_failed')),
  current_node_id TEXT,
  created_at INTEGER NOT NULL,
  started_at INTEGER,
  finished_at INTEGER,
  duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
  error_category TEXT,
  error_message TEXT CHECK (error_message IS NULL OR length(CAST(error_message AS BLOB)) <= 8192),
  retryable INTEGER NOT NULL DEFAULT 0 CHECK (retryable IN (0,1)),
  FOREIGN KEY (workflow_id, workflow_version) REFERENCES workflow_versions(workflow_id, version) ON DELETE RESTRICT
);

CREATE TABLE workflow_node_runs (
  run_id TEXT NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
  node_id TEXT NOT NULL,
  attempt INTEGER NOT NULL CHECK (attempt > 0),
  status TEXT NOT NULL CHECK (status IN ('pending','running','succeeded','failed','cancelled','uncertain','skipped')),
  execution_id TEXT REFERENCES executions(id) ON DELETE SET NULL,
  started_at INTEGER,
  finished_at INTEGER,
  duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
  exit_code INTEGER,
  result_json TEXT CHECK (result_json IS NULL OR length(CAST(result_json AS BLOB)) <= 32768),
  output_summary TEXT CHECK (output_summary IS NULL OR length(CAST(output_summary AS BLOB)) <= 8192),
  error_message TEXT CHECK (error_message IS NULL OR length(CAST(error_message AS BLOB)) <= 8192),
  retryable INTEGER NOT NULL DEFAULT 0 CHECK (retryable IN (0,1)),
  PRIMARY KEY (run_id, node_id, attempt)
);

CREATE TABLE workflow_restore_points (
  id TEXT PRIMARY KEY NOT NULL,
  run_id TEXT NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
  node_id TEXT NOT NULL,
  remote_path TEXT NOT NULL,
  relative_path TEXT,
  original_existed INTEGER NOT NULL CHECK (original_existed IN (0,1)),
  size_bytes INTEGER CHECK (size_bytes IS NULL OR size_bytes >= 0),
  sha256 TEXT CHECK (sha256 IS NULL OR length(sha256) = 64),
  status TEXT NOT NULL CHECK (status IN ('creating','available','failed','rolling_back','rolled_back','expired')),
  applicability_json TEXT NOT NULL,
  error_message TEXT CHECK (error_message IS NULL OR length(CAST(error_message AS BLOB)) <= 8192),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE workflow_run_events (
  run_id TEXT NOT NULL REFERENCES workflow_runs(id) ON DELETE CASCADE,
  sequence INTEGER NOT NULL CHECK (sequence > 0),
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL CHECK (length(CAST(payload_json AS BLOB)) <= 32768),
  emitted_at INTEGER NOT NULL,
  PRIMARY KEY (run_id, sequence)
);

CREATE INDEX idx_workflows_updated ON workflows(updated_at DESC);
CREATE INDEX idx_workflow_runs_created ON workflow_runs(created_at DESC);
CREATE INDEX idx_workflow_runs_server_status ON workflow_runs(server_id, status, created_at DESC);
CREATE INDEX idx_workflow_node_runs_run ON workflow_node_runs(run_id, node_id, attempt);
CREATE INDEX idx_workflow_restore_points_run ON workflow_restore_points(run_id, created_at DESC);
