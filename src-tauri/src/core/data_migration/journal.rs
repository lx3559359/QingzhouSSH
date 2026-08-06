use std::{
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    core::{
        data_migration::{DataMigrationManifest, MIGRATION_JOURNAL_FILE},
        data_root::DataRootSource,
    },
    error::{AppError, AppResult},
};

const JOURNAL_SCHEMA_VERSION: u32 = 1;
const MAX_ERROR_CHARS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataMigrationPhase {
    Prepared,
    Copying,
    Verifying,
    Switched,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DataMigrationJournal {
    pub schema_version: u32,
    pub migration_id: Uuid,
    pub source: PathBuf,
    pub target: PathBuf,
    pub source_mode: DataRootSource,
    pub parent_pid: u32,
    pub file_count: u64,
    pub total_bytes: u64,
    pub copied_files: u64,
    pub copied_bytes: u64,
    pub phase: DataMigrationPhase,
    pub error_summary: Option<String>,
    pub started_at: i64,
    pub updated_at: i64,
    pub acknowledged: bool,
}

impl DataMigrationJournal {
    pub fn prepared(
        source: PathBuf,
        target: PathBuf,
        source_mode: DataRootSource,
        parent_pid: u32,
        manifest: &DataMigrationManifest,
        now: i64,
    ) -> Self {
        Self {
            schema_version: JOURNAL_SCHEMA_VERSION,
            migration_id: Uuid::new_v4(),
            source,
            target,
            source_mode,
            parent_pid,
            file_count: manifest.file_count,
            total_bytes: manifest.total_bytes,
            copied_files: 0,
            copied_bytes: 0,
            phase: DataMigrationPhase::Prepared,
            error_summary: None,
            started_at: now,
            updated_at: now,
            acknowledged: false,
        }
    }

    pub fn transition(&mut self, phase: DataMigrationPhase, now: i64) {
        self.phase = phase;
        self.updated_at = now;
        if phase != DataMigrationPhase::Failed {
            self.error_summary = None;
        }
    }

    pub fn fail(&mut self, error: &AppError, now: i64) {
        self.phase = DataMigrationPhase::Failed;
        self.updated_at = now;
        self.error_summary = Some(error.to_string().chars().take(MAX_ERROR_CHARS).collect());
    }

    pub fn validate(&self) -> AppResult<()> {
        if self.schema_version != JOURNAL_SCHEMA_VERSION {
            return Err(AppError::Validation("数据迁移事务版本不受支持".into()));
        }
        if !self.source.is_absolute() || !self.target.is_absolute() || self.source == self.target {
            return Err(AppError::Security("数据迁移事务路径无效".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct MigrationJournalStore {
    source_path: PathBuf,
    target_path: PathBuf,
}

impl MigrationJournalStore {
    pub fn new(source: &Path, target: &Path) -> Self {
        Self {
            source_path: source.join(MIGRATION_JOURNAL_FILE),
            target_path: target.join(MIGRATION_JOURNAL_FILE),
        }
    }

    pub fn save(&self, journal: &DataMigrationJournal) -> AppResult<()> {
        journal.validate()?;
        save_atomic(&self.source_path, journal)?;
        if self.target_path.parent().is_some_and(Path::exists) {
            save_atomic(&self.target_path, journal)?;
        }
        Ok(())
    }

    pub fn load(path: &Path) -> AppResult<DataMigrationJournal> {
        let payload = std::fs::read(path)?;
        let journal: DataMigrationJournal = serde_json::from_slice(&payload)
            .map_err(|error| AppError::Serialization(error.to_string()))?;
        journal.validate()?;
        Ok(journal)
    }

    pub fn source_path(&self) -> &Path {
        &self.source_path
    }
}

fn save_atomic(path: &Path, journal: &DataMigrationJournal) -> AppResult<()> {
    let payload = serde_json::to_vec_pretty(journal)
        .map_err(|error| AppError::Serialization(error.to_string()))?;
    AtomicFile::new(path, AllowOverwrite)
        .write(|file| {
            let mut writer = BufWriter::new(file);
            writer.write_all(&payload)?;
            writer.flush()?;
            writer.get_ref().sync_all()
        })
        .map_err(|error| AppError::Io(std::io::Error::other(error.to_string())))
}
