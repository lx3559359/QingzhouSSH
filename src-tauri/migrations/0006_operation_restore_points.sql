PRAGMA foreign_keys = ON;

CREATE TABLE operation_restore_points (
  id TEXT PRIMARY KEY NOT NULL,
  operation_run_id TEXT NOT NULL UNIQUE REFERENCES operation_runs(id) ON DELETE RESTRICT,
  server_id TEXT NOT NULL REFERENCES servers(id) ON DELETE RESTRICT,
  task_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN (
    'creating','available','rolling_back','rolled_back','partial','failed','expired','cleanup_pending'
  )),
  local_relative_dir TEXT NOT NULL CHECK (
    length(CAST(local_relative_dir AS BLOB)) BETWEEN 1 AND 2048
    AND substr(local_relative_dir, 1, 1) <> '/'
    AND instr(local_relative_dir, char(92)) = 0
    AND instr(local_relative_dir, ':') = 0
    AND local_relative_dir <> '..'
    AND local_relative_dir NOT LIKE '../%'
    AND local_relative_dir NOT LIKE '%/../%'
  ),
  remote_asset_id TEXT CHECK (remote_asset_id IS NULL OR length(CAST(remote_asset_id AS BLOB)) <= 512),
  expires_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE operation_restore_items (
  id TEXT PRIMARY KEY NOT NULL,
  restore_point_id TEXT NOT NULL REFERENCES operation_restore_points(id) ON DELETE RESTRICT,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0 AND ordinal < 1000),
  item_kind TEXT NOT NULL CHECK (item_kind IN ('remote_file','command_snapshot','managed_block','runtime_state')),
  remote_target TEXT NOT NULL CHECK (length(CAST(remote_target AS BLOB)) BETWEEN 1 AND 4096),
  local_relative_path TEXT CHECK (
    local_relative_path IS NULL OR (
      length(CAST(local_relative_path AS BLOB)) BETWEEN 1 AND 4096
      AND substr(local_relative_path, 1, 1) <> '/'
      AND instr(local_relative_path, char(92)) = 0
      AND instr(local_relative_path, ':') = 0
      AND local_relative_path <> '..'
      AND local_relative_path NOT LIKE '../%'
      AND local_relative_path NOT LIKE '%/../%'
    )
  ),
  sha256 TEXT CHECK (sha256 IS NULL OR (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9A-Fa-f]*')),
  original_metadata_json TEXT NOT NULL CHECK (length(CAST(original_metadata_json AS BLOB)) <= 16384),
  status TEXT NOT NULL CHECK (status IN ('pending','available','rolling_back','rolled_back','failed','skipped')),
  error_summary TEXT CHECK (error_summary IS NULL OR length(CAST(error_summary AS BLOB)) <= 8192),
  UNIQUE (restore_point_id, ordinal)
);

CREATE INDEX idx_operation_restore_points_run ON operation_restore_points(operation_run_id, created_at DESC);
CREATE INDEX idx_operation_restore_points_status ON operation_restore_points(status, expires_at);
CREATE INDEX idx_operation_restore_items_point ON operation_restore_items(restore_point_id, ordinal);
