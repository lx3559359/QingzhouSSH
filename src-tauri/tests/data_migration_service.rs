use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use qingzhou_ssh_lib::{
    core::{
        data_migration::DataMigrationPreview,
        data_root::{initialize_data_root, DataRootSource},
    },
    error::AppResult,
    services::data_migration_service::{
        DataMigrationPreviewRegistry, DataMigrationService, MigrationWorkerSpawner,
    },
};
use uuid::Uuid;

#[derive(Default)]
struct Spawner {
    paths: Mutex<Vec<PathBuf>>,
}

impl MigrationWorkerSpawner for Spawner {
    fn spawn(&self, journal_path: &Path) -> AppResult<()> {
        self.paths.lock().unwrap().push(journal_path.to_path_buf());
        Ok(())
    }
}

fn source(parent: &Path) -> PathBuf {
    let root = parent.join("source");
    initialize_data_root(&root).unwrap();
    fs::write(root.join("app.db"), b"database").unwrap();
    root
}

#[tokio::test]
async fn confirmation_is_one_time_and_spawns_only_after_revalidation() {
    let temp = tempfile::tempdir().unwrap();
    let source = source(temp.path());
    let target = temp.path().join("target");
    let spawner = Arc::new(Spawner::default());
    let service = DataMigrationService::new_with_spawner(source, spawner.clone());

    let preview = service
        .preflight(&target, DataRootSource::Registry, true)
        .await
        .unwrap();
    let journal = service
        .start(
            preview.preview_id,
            preview.confirmation_token,
            DataRootSource::Registry,
            true,
        )
        .await
        .unwrap();
    assert_eq!(spawner.paths.lock().unwrap().len(), 1);
    assert_eq!(journal.target, fs::canonicalize(target).unwrap());
    assert!(service
        .start(
            preview.preview_id,
            preview.confirmation_token,
            DataRootSource::Registry,
            true,
        )
        .await
        .is_err());
}

#[tokio::test]
async fn source_or_target_changes_and_environment_lock_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let source = source(temp.path());
    let target = temp.path().join("target");
    let spawner = Arc::new(Spawner::default());
    let service = DataMigrationService::new_with_spawner(source.clone(), spawner.clone());

    assert!(service
        .preflight(&target, DataRootSource::Environment, false)
        .await
        .is_err());

    let preview = service
        .preflight(&target, DataRootSource::Registry, true)
        .await
        .unwrap();
    fs::write(source.join("logs/after-preview.log"), b"changed").unwrap();
    assert!(service
        .start(
            preview.preview_id,
            preview.confirmation_token,
            DataRootSource::Registry,
            true,
        )
        .await
        .is_err());

    fs::remove_file(source.join("logs/after-preview.log")).unwrap();
    let preview = service
        .preflight(&target, DataRootSource::Registry, true)
        .await
        .unwrap();
    fs::write(target.join("foreign.txt"), b"keep").unwrap();
    assert!(service
        .start(
            preview.preview_id,
            preview.confirmation_token,
            DataRootSource::Registry,
            true,
        )
        .await
        .is_err());
    assert!(spawner.paths.lock().unwrap().is_empty());
    assert_eq!(fs::read(target.join("foreign.txt")).unwrap(), b"keep");
}

#[tokio::test]
async fn failed_migration_can_retry_only_its_recorded_partial_target() {
    let temp = tempfile::tempdir().unwrap();
    let source = source(temp.path());
    let target = temp.path().join("target");
    let spawner = Arc::new(Spawner::default());
    let service = DataMigrationService::new_with_spawner(source.clone(), spawner.clone());
    let (_preview, manifest) =
        qingzhou_ssh_lib::core::data_migration::preflight_data_root_migration(&source, &target, 1)
            .unwrap();
    let mut failed = qingzhou_ssh_lib::core::data_migration::DataMigrationJournal::prepared(
        fs::canonicalize(&source).unwrap(),
        fs::canonicalize(&target).unwrap(),
        DataRootSource::Registry,
        42,
        &manifest,
        1,
    );
    failed.phase = qingzhou_ssh_lib::core::data_migration::DataMigrationPhase::Failed;
    fs::write(target.join("app.db"), b"partial").unwrap();
    qingzhou_ssh_lib::core::data_migration::MigrationJournalStore::new(&source, &target)
        .save(&failed)
        .unwrap();

    let retry = service
        .preflight_retry(DataRootSource::Registry, true)
        .await
        .unwrap();
    assert!(retry.retryable);
    assert_eq!(retry.target, fs::canonicalize(&target).unwrap());
    service
        .start(
            retry.preview_id,
            retry.confirmation_token,
            DataRootSource::Registry,
            true,
        )
        .await
        .unwrap();
    assert_eq!(spawner.paths.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn preview_tokens_expire_and_wrong_tokens_do_not_consume_the_preview() {
    let registry = DataMigrationPreviewRegistry::default();
    let preview = DataMigrationPreview {
        preview_id: Uuid::new_v4(),
        confirmation_token: Uuid::new_v4(),
        expires_at: 200,
        source: r"D:\old".into(),
        target: r"E:\new".into(),
        file_count: 1,
        total_bytes: 1,
        required_bytes: 64 * 1024 * 1024,
        available_bytes: u64::MAX,
        old_root_will_be_kept: true,
        retryable: false,
    };
    registry
        .issue(preview.clone(), DataRootSource::Registry)
        .await;
    assert!(registry
        .consume(preview.preview_id, Uuid::new_v4(), 100)
        .await
        .is_err());
    assert!(registry
        .consume(preview.preview_id, preview.confirmation_token, 100)
        .await
        .is_ok());

    let expired = DataMigrationPreview {
        preview_id: Uuid::new_v4(),
        confirmation_token: Uuid::new_v4(),
        ..preview
    };
    registry
        .issue(expired.clone(), DataRootSource::Registry)
        .await;
    assert!(registry
        .consume(expired.preview_id, expired.confirmation_token, 200)
        .await
        .is_err());
}
