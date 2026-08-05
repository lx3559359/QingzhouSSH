PRAGMA foreign_keys = ON;

CREATE TABLE operation_runs (
  id TEXT PRIMARY KEY NOT NULL,
  server_id TEXT NOT NULL REFERENCES servers(id) ON DELETE RESTRICT,
  task_id TEXT NOT NULL,
  task_version INTEGER NOT NULL CHECK (task_version > 0),
  risk_level TEXT NOT NULL CHECK (risk_level IN ('safe','caution','dangerous')),
  status TEXT NOT NULL CHECK (status IN (
    'validating','preflighting','preview_ready','waiting_confirmation',
    'backing_up','running','verifying','succeeded','failed','cancelled',
    'uncertain','rollback_available','rolling_back','rolled_back',
    'rollback_partial','rollback_failed'
  )),
  parameters_summary TEXT CHECK (parameters_summary IS NULL OR length(CAST(parameters_summary AS BLOB)) <= 8192),
  result_json TEXT CHECK (result_json IS NULL OR length(CAST(result_json AS BLOB)) <= 65536),
  error_category TEXT,
  error_message TEXT CHECK (error_message IS NULL OR length(CAST(error_message AS BLOB)) <= 8192),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  finished_at INTEGER
);

CREATE TABLE operation_steps (
  run_id TEXT NOT NULL REFERENCES operation_runs(id) ON DELETE CASCADE,
  phase TEXT NOT NULL CHECK (phase IN ('preflight','backup','execute','verify','rollback')),
  step_index INTEGER NOT NULL CHECK (step_index >= 0),
  step_id TEXT NOT NULL,
  title TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending','running','succeeded','failed','cancelled','uncertain','skipped')),
  execution_id TEXT REFERENCES executions(id) ON DELETE SET NULL,
  output_summary TEXT CHECK (output_summary IS NULL OR length(CAST(output_summary AS BLOB)) <= 8192),
  error_message TEXT CHECK (error_message IS NULL OR length(CAST(error_message AS BLOB)) <= 8192),
  started_at INTEGER,
  finished_at INTEGER,
  PRIMARY KEY (run_id, phase, step_index)
);

CREATE INDEX idx_operation_runs_created ON operation_runs(created_at DESC);
CREATE INDEX idx_operation_runs_server_status ON operation_runs(server_id,status,created_at DESC);
