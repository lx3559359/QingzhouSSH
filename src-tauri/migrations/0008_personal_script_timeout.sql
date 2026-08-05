ALTER TABLE script_versions
ADD COLUMN timeout_seconds INTEGER NOT NULL DEFAULT 300
CHECK (timeout_seconds BETWEEN 1 AND 3600);
