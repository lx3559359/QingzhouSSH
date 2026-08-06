use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};

use atomicwrites::{AllowOverwrite, AtomicFile};

use crate::{
    core::data_migration::{
        hash_file, scan_manifest, DataMigrationJournal, DataMigrationManifest,
        DataMigrationManifestEntry, DataMigrationPhase, ManifestEntryKind, MigrationEnvironment,
        MigrationJournalStore,
    },
    domain::execution::now_millis,
    error::{AppError, AppResult},
};

#[derive(Debug, Clone)]
pub struct VerifiedMigration {
    migration_id: uuid::Uuid,
    source: PathBuf,
    target: PathBuf,
    file_count: u64,
    total_bytes: u64,
}

impl VerifiedMigration {
    pub fn migration_id(&self) -> uuid::Uuid {
        self.migration_id
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn target(&self) -> &Path {
        &self.target
    }

    pub fn file_count(&self) -> u64 {
        self.file_count
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
}

pub struct CopyAndVerifyRequest<'a, E> {
    source: &'a Path,
    target: &'a Path,
    manifest: &'a DataMigrationManifest,
    retryable: bool,
    store: &'a MigrationJournalStore,
    environment: &'a E,
}

impl<'a, E> CopyAndVerifyRequest<'a, E> {
    pub fn new(
        source: &'a Path,
        target: &'a Path,
        manifest: &'a DataMigrationManifest,
        retryable: bool,
        store: &'a MigrationJournalStore,
        environment: &'a E,
    ) -> Self {
        Self {
            source,
            target,
            manifest,
            retryable,
            store,
            environment,
        }
    }
}

pub fn copy_and_verify<E, F>(
    request: CopyAndVerifyRequest<'_, E>,
    journal: &mut DataMigrationJournal,
    before_verify: F,
) -> AppResult<VerifiedMigration>
where
    E: MigrationEnvironment,
    F: FnOnce(&Path) -> AppResult<()>,
{
    let result = copy_and_verify_inner(&request, journal, before_verify);
    if let Err(error) = &result {
        journal.fail(error, now_millis());
        let _ = request.store.save(journal);
    }
    result
}

fn copy_and_verify_inner<E, F>(
    request: &CopyAndVerifyRequest<'_, E>,
    journal: &mut DataMigrationJournal,
    before_verify: F,
) -> AppResult<VerifiedMigration>
where
    E: MigrationEnvironment,
    F: FnOnce(&Path) -> AppResult<()>,
{
    let source = request.source;
    let target = request.target;
    let manifest = request.manifest;
    let retryable = request.retryable;
    let store = request.store;
    let environment = request.environment;
    validate_existing_target(target, manifest, retryable, environment)?;
    journal.transition(DataMigrationPhase::Copying, now_millis());
    store.save(journal)?;

    for entry in &manifest.entries {
        let source_path = source.join(&entry.relative_path);
        let target_path = target.join(&entry.relative_path);
        match entry.kind {
            ManifestEntryKind::Directory => fs::create_dir_all(&target_path)?,
            ManifestEntryKind::File => {
                if should_copy_file(&target_path, entry, retryable)? {
                    if let Some(parent) = target_path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    copy_file_atomic(&source_path, &target_path)?;
                }
                journal.copied_files = journal.copied_files.saturating_add(1);
                journal.copied_bytes = journal.copied_bytes.saturating_add(entry.size_bytes);
                journal.updated_at = now_millis();
                store.save(journal)?;
            }
        }
    }

    journal.transition(DataMigrationPhase::Verifying, now_millis());
    store.save(journal)?;
    before_verify(target)?;
    verify_manifest(target, manifest, environment)?;
    Ok(VerifiedMigration {
        migration_id: journal.migration_id,
        source: source.to_path_buf(),
        target: target.to_path_buf(),
        file_count: manifest.file_count,
        total_bytes: manifest.total_bytes,
    })
}

pub fn verify_manifest<E: MigrationEnvironment>(
    target: &Path,
    expected: &DataMigrationManifest,
    environment: &E,
) -> AppResult<()> {
    let actual = scan_manifest(target, environment)?;
    if actual != *expected {
        return Err(AppError::Integrity(
            "目标目录的文件路径、大小或 SHA-256 与源清单不一致".into(),
        ));
    }
    Ok(())
}

fn validate_existing_target<E: MigrationEnvironment>(
    target: &Path,
    manifest: &DataMigrationManifest,
    retryable: bool,
    environment: &E,
) -> AppResult<()> {
    let existing = scan_manifest(target, environment)?;
    if existing.entries.is_empty() {
        return Ok(());
    }
    if !retryable {
        return Err(AppError::Validation(
            "目标目录在确认后出现了文件，请重新选择空目录".into(),
        ));
    }
    for entry in existing.entries {
        let Some(expected) = manifest
            .entries
            .iter()
            .find(|candidate| candidate.relative_path == entry.relative_path)
        else {
            return Err(AppError::Security("失败目标包含迁移清单以外的文件".into()));
        };
        if expected.kind != entry.kind {
            return Err(AppError::Security("失败目标中的文件类型发生变化".into()));
        }
    }
    Ok(())
}

fn should_copy_file(
    target_path: &Path,
    entry: &DataMigrationManifestEntry,
    retryable: bool,
) -> AppResult<bool> {
    if !target_path.exists() {
        return Ok(true);
    }
    if !retryable {
        return Err(AppError::Validation("目标文件已存在".into()));
    }
    let (size, hash) = hash_file(target_path)?;
    Ok(size != entry.size_bytes || Some(hash) != entry.sha256)
}

fn copy_file_atomic(source: &Path, target: &Path) -> AppResult<()> {
    AtomicFile::new(target, AllowOverwrite)
        .write(|destination| {
            let mut source = File::open(source)?;
            io::copy(&mut source, destination)?;
            destination.sync_all()
        })
        .map_err(|error| AppError::Io(std::io::Error::other(error.to_string())))
}
