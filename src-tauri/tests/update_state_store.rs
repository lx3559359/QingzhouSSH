use std::time::Duration;

use qingzhou_ssh_lib::{
    core::updates::{
        StoredCheckResult, StoredCheckStatus, UpdatePersistentState, UpdateStateStore,
    },
    domain::update::UpdateSource,
};
use tempfile::tempdir;

#[test]
fn atomically_persists_safe_update_preferences() {
    let root = tempdir().unwrap();
    let store = UpdateStateStore::new(root.path()).unwrap();
    let mut state = store.load().unwrap();
    assert!(state.auto_check);
    assert!(state.last_checked_at.is_none());

    state.auto_check = false;
    state.last_checked_at = Some(123_456);
    state.last_result = Some(StoredCheckResult {
        status: StoredCheckStatus::Available,
        version: Some("0.2.0".into()),
        source: Some(UpdateSource::Github),
        message: Some("发现新版本".into()),
    });
    state.staged_file = Some("staged/0.2.0/QingzhouSSH.exe".into());
    store.save(&state).unwrap();

    assert_eq!(store.load().unwrap(), state);
    assert!(root.path().join("updates/state.json").is_file());
    assert!(!root.path().join("updates/state.json.partial").exists());
}

#[test]
fn rejects_absolute_or_traversing_staged_paths() {
    let root = tempdir().unwrap();
    let store = UpdateStateStore::new(root.path()).unwrap();
    for path in [
        r"C:\temp\update.exe",
        "../private.key",
        "staged/../../escape",
    ] {
        let state = UpdatePersistentState {
            staged_file: Some(path.into()),
            ..UpdatePersistentState::default()
        };
        assert!(store.save(&state).is_err(), "accepted {path}");
    }
}

#[test]
fn rate_limits_only_automatic_checks() {
    let state = UpdatePersistentState {
        auto_check: true,
        last_checked_at: Some(1_000),
        ..UpdatePersistentState::default()
    };
    assert!(!state.automatic_check_due(1_100, Duration::from_secs(200)));
    assert!(state.automatic_check_due(1_201, Duration::from_secs(200)));

    let disabled = UpdatePersistentState {
        auto_check: false,
        ..UpdatePersistentState::default()
    };
    assert!(!disabled.automatic_check_due(u64::MAX, Duration::from_secs(1)));
}

#[test]
fn removes_only_partial_files_inside_the_update_directory() {
    let root = tempdir().unwrap();
    let store = UpdateStateStore::new(root.path()).unwrap();
    let staged = root.path().join("updates/staged/0.2.0");
    std::fs::create_dir_all(&staged).unwrap();
    std::fs::write(staged.join("package.partial"), b"incomplete").unwrap();
    std::fs::write(staged.join("package.exe"), b"keep").unwrap();
    std::fs::write(root.path().join("outside.partial"), b"keep outside").unwrap();

    assert_eq!(store.cleanup_partial_files().unwrap(), 1);
    assert!(!staged.join("package.partial").exists());
    assert!(staged.join("package.exe").exists());
    assert!(root.path().join("outside.partial").exists());
}
