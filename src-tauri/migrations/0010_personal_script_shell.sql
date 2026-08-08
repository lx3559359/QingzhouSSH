ALTER TABLE script_versions
ADD COLUMN shell TEXT NOT NULL DEFAULT 'posix_sh'
CHECK (shell IN ('posix_sh', 'bash', 'powershell'));

ALTER TABLE script_versions
ADD COLUMN compatibility_json TEXT NOT NULL DEFAULT '{"osFamilies":["linux","bsd"],"requiredCommands":["sh"]}'
CHECK (length(CAST(compatibility_json AS BLOB)) <= 16384 AND json_valid(compatibility_json));
