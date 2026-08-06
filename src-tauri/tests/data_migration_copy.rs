use std::{cell::Cell, fs, path::Path};

use qingzhou_ssh_lib::core::{
    data_migration::{
        copy_and_verify, preflight_with, DataMigrationJournal, DataMigrationPhase,
        MigrationEnvironment, MigrationJournalStore,
    },
    data_root::{initialize_data_root, DataRootSource},
};
use qingzhou_ssh_lib::error::AppResult;

struct Environment;

impl MigrationEnvironment for Environment {
    fn is_reparse_point(&self, _path: &Path) -> AppResult<bool> {
        Ok(false)
    }
    fn probe_writable(&self, _directory: &Path) -> AppResult<()> {
        Ok(())
    }
    fn available_space(&self, _directory: &Path) -> AppResult<u64> {
        Ok(u64::MAX)
    }
}

fn fixture(parent: &Path) -> std::path::PathBuf {
    let source = parent.join("source");
    initialize_data_root(&source).unwrap();
    fs::write(source.join("app.db"), b"database").unwrap();
    fs::create_dir_all(source.join("logs/empty")).unwrap();
    fs::create_dir_all(source.join("downloads/nested")).unwrap();
    fs::write(source.join("downloads/nested/report.txt"), b"report").unwrap();
    source
}

fn prepared(
    source: &Path,
    target: &Path,
) -> (
    qingzhou_ssh_lib::core::data_migration::DataMigrationManifest,
    DataMigrationJournal,
    MigrationJournalStore,
) {
    let (_preview, manifest) = preflight_with(source, target, 1, &Environment).unwrap();
    let journal = DataMigrationJournal::prepared(
        fs::canonicalize(source).unwrap(),
        fs::canonicalize(target).unwrap(),
        DataRootSource::Registry,
        std::process::id(),
        &manifest,
        1,
    );
    let store = MigrationJournalStore::new(source, target);
    (manifest, journal, store)
}

#[test]
fn copies_files_and_empty_directories_then_verifies_sha256() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path());
    let target = temp.path().join("target");
    let (manifest, mut journal, store) = prepared(&source, &target);
    store.save(&journal).unwrap();

    let verified = copy_and_verify(
        &source,
        &target,
        &manifest,
        false,
        &mut journal,
        &store,
        &Environment,
        |_| Ok(()),
    )
    .unwrap();

    assert_eq!(verified.file_count(), 2);
    assert_eq!(fs::read(target.join("app.db")).unwrap(), b"database");
    assert_eq!(
        fs::read(target.join("downloads/nested/report.txt")).unwrap(),
        b"report"
    );
    assert!(target.join("logs/empty").is_dir());
    assert_eq!(fs::read(source.join("app.db")).unwrap(), b"database");
    assert_eq!(journal.phase, DataMigrationPhase::Verifying);
}

#[test]
fn corruption_marks_failed_and_never_qualifies_for_pointer_switch() {
    let temp = tempfile::tempdir().unwrap();
    let source = fixture(temp.path());
    let target = temp.path().join("target");
    let (manifest, mut journal, store) = prepared(&source, &target);
    store.save(&journal).unwrap();
    let pointer_switches = Cell::new(0_u32);

    let result = copy_and_verify(
        &source,
        &target,
        &manifest,
        false,
        &mut journal,
        &store,
        &Environment,
        |target| {
            fs::write(target.join("app.db"), b"tampered")?;
            Ok(())
        },
    );
    if result.is_ok() {
        pointer_switches.set(pointer_switches.get() + 1);
    }

    assert!(result.is_err());
    assert_eq!(pointer_switches.get(), 0);
    assert_eq!(journal.phase, DataMigrationPhase::Failed);
    assert_eq!(fs::read(source.join("app.db")).unwrap(), b"database");
    let persisted = MigrationJournalStore::load(store.source_path()).unwrap();
    assert_eq!(persisted.phase, DataMigrationPhase::Failed);
}
