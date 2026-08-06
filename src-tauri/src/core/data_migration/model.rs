use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MIGRATION_JOURNAL_FILE: &str = ".qingzhou-data-migration.json";
pub const MIGRATION_COMPLETE_FILE: &str = ".qingzhou-data-migration-complete.json";
pub const ALLOWED_ROOT_ENTRIES: &[&str] = &[
    "app.db",
    "app.db-wal",
    "app.db-shm",
    "vault",
    "logs",
    "downloads",
    "backups",
    "templates",
    "cache",
    "updates",
    MIGRATION_JOURNAL_FILE,
    MIGRATION_COMPLETE_FILE,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataMigrationManifestEntry {
    pub relative_path: PathBuf,
    pub kind: ManifestEntryKind,
    pub size_bytes: u64,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataMigrationManifest {
    pub entries: Vec<DataMigrationManifestEntry>,
    pub file_count: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataMigrationPreview {
    pub preview_id: Uuid,
    pub confirmation_token: Uuid,
    pub expires_at: i64,
    pub source: PathBuf,
    pub target: PathBuf,
    pub file_count: u64,
    pub total_bytes: u64,
    pub required_bytes: u64,
    pub available_bytes: u64,
    pub old_root_will_be_kept: bool,
    pub retryable: bool,
}
