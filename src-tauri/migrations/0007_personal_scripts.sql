PRAGMA foreign_keys = ON;

CREATE TABLE script_definitions (
  id TEXT PRIMARY KEY NOT NULL,
  title TEXT NOT NULL CHECK (length(CAST(title AS BLOB)) BETWEEN 1 AND 240),
  category TEXT NOT NULL CHECK (length(CAST(category AS BLOB)) BETWEEN 1 AND 120),
  tags_json TEXT NOT NULL CHECK (length(CAST(tags_json AS BLOB)) <= 8192 AND json_valid(tags_json)),
  is_favorite INTEGER NOT NULL DEFAULT 0 CHECK (is_favorite IN (0, 1)),
  is_enabled INTEGER NOT NULL DEFAULT 1 CHECK (is_enabled IN (0, 1)),
  active_version_id TEXT REFERENCES script_versions(id) ON DELETE RESTRICT DEFERRABLE INITIALLY DEFERRED,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER
);

CREATE TABLE script_versions (
  id TEXT PRIMARY KEY NOT NULL,
  definition_id TEXT NOT NULL REFERENCES script_definitions(id) ON DELETE RESTRICT,
  version_number INTEGER NOT NULL CHECK (version_number BETWEEN 1 AND 2147483647),
  body TEXT NOT NULL CHECK (length(CAST(body AS BLOB)) BETWEEN 1 AND 1048576 AND instr(body, char(0)) = 0),
  body_sha256 TEXT NOT NULL CHECK (length(body_sha256) = 64 AND body_sha256 NOT GLOB '*[^0-9a-f]*'),
  parameters_json TEXT NOT NULL CHECK (length(CAST(parameters_json AS BLOB)) <= 131072 AND json_valid(parameters_json)),
  scan_summary_json TEXT NOT NULL CHECK (length(CAST(scan_summary_json AS BLOB)) <= 65536 AND json_valid(scan_summary_json)),
  created_at INTEGER NOT NULL,
  UNIQUE (definition_id, version_number),
  UNIQUE (definition_id, id)
);

CREATE TABLE script_runs (
  id TEXT PRIMARY KEY NOT NULL,
  definition_id TEXT NOT NULL REFERENCES script_definitions(id) ON DELETE RESTRICT,
  version_id TEXT NOT NULL,
  operation_run_id TEXT NOT NULL UNIQUE REFERENCES operation_runs(id) ON DELETE RESTRICT,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (definition_id, version_id) REFERENCES script_versions(definition_id, id) ON DELETE RESTRICT
);

CREATE TRIGGER script_versions_forbid_update
BEFORE UPDATE ON script_versions
BEGIN
  SELECT RAISE(ABORT, 'script versions are immutable');
END;

CREATE TRIGGER script_versions_forbid_delete
BEFORE DELETE ON script_versions
BEGIN
  SELECT RAISE(ABORT, 'script versions are immutable');
END;

CREATE INDEX idx_script_definitions_list ON script_definitions(deleted_at, is_enabled, is_favorite, updated_at DESC);
CREATE INDEX idx_script_versions_definition ON script_versions(definition_id, version_number DESC);
CREATE INDEX idx_script_runs_definition ON script_runs(definition_id, created_at DESC);
