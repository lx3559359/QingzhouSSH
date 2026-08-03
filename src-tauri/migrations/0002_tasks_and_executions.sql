PRAGMA foreign_keys = ON;

CREATE TABLE task_definitions (
  id TEXT NOT NULL,
  version INTEGER NOT NULL CHECK (version > 0),
  category TEXT NOT NULL CHECK (category IN ('system', 'service', 'logs', 'advanced', 'transfer')),
  definition_json TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (id, version)
);

CREATE TABLE executions (
  id TEXT PRIMARY KEY NOT NULL,
  server_id TEXT NOT NULL REFERENCES servers(id) ON DELETE RESTRICT,
  task_id TEXT NOT NULL,
  task_version INTEGER NOT NULL CHECK (task_version > 0),
  category TEXT NOT NULL CHECK (category IN ('system', 'service', 'logs', 'advanced', 'transfer')),
  status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled', 'uncertain')),
  created_at INTEGER NOT NULL,
  started_at INTEGER,
  finished_at INTEGER,
  duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
  exit_code INTEGER,
  error_category TEXT,
  error_message TEXT CHECK (error_message IS NULL OR length(CAST(error_message AS BLOB)) <= 8192),
  retryable INTEGER NOT NULL DEFAULT 0 CHECK (retryable IN (0, 1)),
  parameters_summary TEXT CHECK (parameters_summary IS NULL OR length(CAST(parameters_summary AS BLOB)) <= 8192),
  output_summary TEXT CHECK (output_summary IS NULL OR length(CAST(output_summary AS BLOB)) <= 8192),
  remote_process_group TEXT
);

CREATE TABLE execution_parameters (
  execution_id TEXT NOT NULL REFERENCES executions(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  display_value TEXT NOT NULL CHECK (length(CAST(display_value AS BLOB)) <= 8192),
  sensitive INTEGER NOT NULL DEFAULT 0 CHECK (sensitive IN (0, 1)),
  PRIMARY KEY (execution_id, name)
);

CREATE TABLE execution_files (
  id TEXT PRIMARY KEY NOT NULL,
  execution_id TEXT NOT NULL REFERENCES executions(id) ON DELETE CASCADE,
  relative_path TEXT NOT NULL,
  purpose TEXT NOT NULL,
  size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
  sha256 TEXT NOT NULL CHECK (length(sha256) = 64)
);

CREATE INDEX idx_executions_created_at ON executions(created_at DESC);
CREATE INDEX idx_executions_server_status ON executions(server_id, status, created_at DESC);
CREATE INDEX idx_execution_files_execution ON execution_files(execution_id);
