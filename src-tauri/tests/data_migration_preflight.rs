use std::{collections::HashSet, fs, path::Path};

use qingzhou_ssh_lib::{
    core::{
        data_migration::{preflight_retry_with, preflight_with, MigrationEnvironment},
        data_root::initialize_data_root,
    },
    error::{AppError, AppResult},
};

#[derive(Default)]
struct FakeEnvironment {
    reparse: HashSet<std::path::PathBuf>,
    writable: bool,
    available: u64,
}

impl FakeEnvironment {
    fn ready() -> Self {
        Self {
            reparse: HashSet::new(),
            writable: true,
            available: u64::MAX,
        }
    }
}

impl MigrationEnvironment for FakeEnvironment {
    fn is_reparse_point(&self, path: &Path) -> AppResult<bool> {
        Ok(self.reparse.contains(path))
    }

    fn probe_writable(&self, _directory: &Path) -> AppResult<()> {
        if self.writable {
            Ok(())
        } else {
            Err(AppError::Permission("目标目录不可写".into()))
        }
    }

    fn available_space(&self, _directory: &Path) -> AppResult<u64> {
        Ok(self.available)
    }
}

fn source_fixture(parent: &Path) -> std::path::PathBuf {
    let source = parent.join("source");
    initialize_data_root(&source).unwrap();
    fs::write(source.join("app.db"), b"database").unwrap();
    fs::create_dir_all(source.join("logs/service")).unwrap();
    fs::write(source.join("logs/service/app.log"), b"ready").unwrap();
    source
}

#[test]
fn creates_a_sorted_hashed_preview_for_an_empty_target() {
    let temp = tempfile::tempdir().unwrap();
    let source = source_fixture(temp.path());
    let target = temp.path().join("target");
    let (preview, manifest) =
        preflight_with(&source, &target, 1_000, &FakeEnvironment::ready()).unwrap();

    assert_eq!(preview.file_count, 2);
    assert_eq!(preview.total_bytes, 13);
    assert!(preview.required_bytes >= 64 * 1024 * 1024);
    assert!(preview.old_root_will_be_kept);
    assert!(!preview.retryable);
    assert!(manifest
        .entries
        .windows(2)
        .all(|pair| pair[0].relative_path <= pair[1].relative_path));
    assert!(manifest
        .entries
        .iter()
        .filter(|entry| entry.sha256.is_some())
        .all(|entry| entry.sha256.as_ref().unwrap().len() == 64));
    assert!(source.join("app.db").exists());
}

#[test]
fn rejects_relative_root_same_parent_child_and_nonempty_targets() {
    let temp = tempfile::tempdir().unwrap();
    let source = source_fixture(temp.path());
    let environment = FakeEnvironment::ready();

    assert!(preflight_with(&source, Path::new("relative"), 0, &environment).is_err());
    assert!(preflight_with(&source, &source, 0, &environment).is_err());
    assert!(preflight_with(&source, &source.join("nested"), 0, &environment).is_err());
    assert!(preflight_with(&source, temp.path(), 0, &environment).is_err());

    let nonempty = temp.path().join("nonempty");
    fs::create_dir(&nonempty).unwrap();
    fs::write(nonempty.join("foreign.txt"), b"keep").unwrap();
    assert!(preflight_with(&source, &nonempty, 0, &environment).is_err());
    assert_eq!(fs::read(nonempty.join("foreign.txt")).unwrap(), b"keep");
}

#[test]
fn rejects_reparse_items_unwritable_targets_and_insufficient_space() {
    let temp = tempfile::tempdir().unwrap();
    let source = source_fixture(temp.path());
    let target = temp.path().join("target");
    let child = fs::canonicalize(source.join("logs/service/app.log")).unwrap();

    let mut reparse = FakeEnvironment::ready();
    reparse.reparse.insert(child);
    assert!(preflight_with(&source, &target, 0, &reparse).is_err());

    let unwritable = FakeEnvironment {
        writable: false,
        ..FakeEnvironment::ready()
    };
    assert!(preflight_with(&source, &target, 0, &unwritable).is_err());

    let no_space = FakeEnvironment {
        available: 0,
        ..FakeEnvironment::ready()
    };
    assert!(matches!(
        preflight_with(&source, &target, 0, &no_space),
        Err(AppError::DiskSpace(_))
    ));
}

#[test]
fn rejects_unknown_root_entries_without_deleting_them() {
    let temp = tempfile::tempdir().unwrap();
    let source = source_fixture(temp.path());
    fs::write(source.join("unexpected.bin"), b"keep").unwrap();
    let target = temp.path().join("target");
    assert!(preflight_with(&source, &target, 0, &FakeEnvironment::ready()).is_err());
    assert_eq!(fs::read(source.join("unexpected.bin")).unwrap(), b"keep");
}

#[test]
fn retry_accepts_only_partial_files_from_the_source_manifest() {
    let temp = tempfile::tempdir().unwrap();
    let source = source_fixture(temp.path());
    let target = temp.path().join("target");
    fs::create_dir_all(target.join("logs/service")).unwrap();
    fs::write(target.join("app.db"), b"partial").unwrap();

    let (preview, _) =
        preflight_retry_with(&source, &target, 0, &FakeEnvironment::ready()).unwrap();
    assert!(preview.retryable);

    fs::write(target.join("logs/foreign.log"), b"keep").unwrap();
    assert!(preflight_retry_with(&source, &target, 0, &FakeEnvironment::ready()).is_err());
    assert_eq!(fs::read(target.join("logs/foreign.log")).unwrap(), b"keep");
}
