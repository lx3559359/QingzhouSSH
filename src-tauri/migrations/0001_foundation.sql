PRAGMA foreign_keys = ON;

CREATE TABLE servers (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  host TEXT NOT NULL,
  port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
  username TEXT NOT NULL,
  auth_kind TEXT NOT NULL CHECK (auth_kind IN ('password', 'private_key')),
  credential_id TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE host_keys (
  server_id TEXT PRIMARY KEY NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
  algorithm TEXT NOT NULL,
  fingerprint_sha256 TEXT NOT NULL,
  raw_key_base64 TEXT NOT NULL,
  trusted_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_servers_name ON servers(name COLLATE NOCASE);
