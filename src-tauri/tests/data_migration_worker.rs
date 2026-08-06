use std::{
    cell::Cell,
    fs,
    path::{Path, PathBuf},
};

use qingzhou_ssh_lib::{
    core::{
        data_migration::{
            run_process_mode, run_worker_with, DataMigrationJournal, DataMigrationPhase,
            DataRootPointer, MigrationEnvironment, MigrationJournalStore, ParentProcessWaiter,
            ProcessLauncher, VerifiedMigration, MIGRATION_COMPLETE_FILE,
        },
        data_root::{initialize_data_root, DataRootSource},
    },
    error::{AppError, AppResult},
};

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

struct Waiter<'a> {
    target: &'a Path,
    fail: bool,
    calls: Cell<u32>,
}

impl ParentProcessWaiter for Waiter<'_> {
    fn wait_for_exit(&self, _parent_pid: u32) -> AppResult<()> {
        self.calls.set(self.calls.get() + 1);
        assert!(
            !self.target.join("app.db").exists(),
            "copy started before parent exit"
        );
        if self.fail {
            Err(AppError::Io(std::io::Error::other("wait failed")))
        } else {
            Ok(())
        }
    }
}

struct Pointer<'a> {
    target: &'a Path,
    commits: Cell<u32>,
}

impl DataRootPointer for Pointer<'_> {
    fn commit(&self, verified: &VerifiedMigration) -> AppResult<()> {
        assert_eq!(
            fs::canonicalize(verified.target()).unwrap(),
            fs::canonicalize(self.target).unwrap()
        );
        assert!(
            self.target.join(MIGRATION_COMPLETE_FILE).is_file(),
            "marker must exist before pointer switch"
        );
        self.commits.set(self.commits.get() + 1);
        Ok(())
    }
}

#[derive(Default)]
struct Launcher {
    calls: Cell<u32>,
}

impl ProcessLauncher for Launcher {
    fn restart(&self, _executable: &Path) -> AppResult<()> {
        self.calls.set(self.calls.get() + 1);
        Ok(())
    }
}

fn prepared(parent: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let source = parent.join("source");
    let target = parent.join("target");
    initialize_data_root(&source).unwrap();
    fs::write(source.join("app.db"), b"database").unwrap();
    let (_preview, manifest) =
        qingzhou_ssh_lib::core::data_migration::preflight_with(&source, &target, 1, &Environment)
            .unwrap();
    let journal = DataMigrationJournal::prepared(
        fs::canonicalize(&source).unwrap(),
        fs::canonicalize(&target).unwrap(),
        DataRootSource::Registry,
        42_424,
        &manifest,
        1,
    );
    let store = MigrationJournalStore::new(&source, &target);
    store.save(&journal).unwrap();
    let journal_path = store.source_path().to_path_buf();
    (source, target, journal_path)
}

#[test]
fn waits_then_verifies_marks_and_switches_exactly_once() {
    let temp = tempfile::tempdir().unwrap();
    let (source, target, journal_path) = prepared(temp.path());
    let waiter = Waiter {
        target: &target,
        fail: false,
        calls: Cell::new(0),
    };
    let pointer = Pointer {
        target: &target,
        commits: Cell::new(0),
    };
    let launcher = Launcher::default();

    let phase = run_worker_with(
        &journal_path,
        &temp.path().join("app.exe"),
        &pointer,
        &waiter,
        &launcher,
        &Environment,
        |_| Ok(()),
    )
    .unwrap();

    assert_eq!(phase, DataMigrationPhase::Completed);
    assert_eq!(waiter.calls.get(), 1);
    assert_eq!(pointer.commits.get(), 1);
    assert_eq!(launcher.calls.get(), 1);
    assert_eq!(fs::read(target.join("app.db")).unwrap(), b"database");
    assert_eq!(fs::read(source.join("app.db")).unwrap(), b"database");
}

#[test]
fn wait_or_verification_failure_keeps_the_old_pointer_and_restarts() {
    let temp = tempfile::tempdir().unwrap();
    let (_source, target, journal_path) = prepared(temp.path());
    let waiter = Waiter {
        target: &target,
        fail: false,
        calls: Cell::new(0),
    };
    let pointer = Pointer {
        target: &target,
        commits: Cell::new(0),
    };
    let launcher = Launcher::default();

    let phase = run_worker_with(
        &journal_path,
        &temp.path().join("app.exe"),
        &pointer,
        &waiter,
        &launcher,
        &Environment,
        |target| {
            fs::write(target.join("app.db"), b"tampered")?;
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(phase, DataMigrationPhase::Failed);
    assert_eq!(pointer.commits.get(), 0);
    assert_eq!(launcher.calls.get(), 1);
    let failed = MigrationJournalStore::load(&journal_path).unwrap();
    assert_eq!(failed.phase, DataMigrationPhase::Failed);

    let second = tempfile::tempdir().unwrap();
    let (_source, target, journal_path) = prepared(second.path());
    let failed_waiter = Waiter {
        target: &target,
        fail: true,
        calls: Cell::new(0),
    };
    let pointer = Pointer {
        target: &target,
        commits: Cell::new(0),
    };
    let launcher = Launcher::default();
    let phase = run_worker_with(
        &journal_path,
        &second.path().join("app.exe"),
        &pointer,
        &failed_waiter,
        &launcher,
        &Environment,
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(phase, DataMigrationPhase::Failed);
    assert_eq!(pointer.commits.get(), 0);
    assert!(!target.join("app.db").exists());
}

#[test]
fn internal_process_mode_rejects_malformed_arguments() {
    assert!(!run_process_mode(["app.exe"]).unwrap());
    assert!(run_process_mode(["app.exe", "--migrate-data-root"]).is_err());
    assert!(run_process_mode(["app.exe", "--migrate-data-root", "relative.json"]).is_err());
}

#[test]
fn worker_uses_the_authoritative_manifest_after_the_parent_exits() {
    let temp = tempfile::tempdir().unwrap();
    let (source, target, journal_path) = prepared(temp.path());
    fs::write(source.join("logs/closed-after-preview.log"), b"final state").unwrap();
    let waiter = Waiter {
        target: &target,
        fail: false,
        calls: Cell::new(0),
    };
    let pointer = Pointer {
        target: &target,
        commits: Cell::new(0),
    };
    let launcher = Launcher::default();

    let phase = run_worker_with(
        &journal_path,
        &temp.path().join("app.exe"),
        &pointer,
        &waiter,
        &launcher,
        &Environment,
        |_| Ok(()),
    )
    .unwrap();

    assert_eq!(phase, DataMigrationPhase::Completed);
    assert_eq!(
        fs::read(target.join("logs/closed-after-preview.log")).unwrap(),
        b"final state"
    );
}
