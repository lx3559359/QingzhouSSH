use std::path::PathBuf;

use qingzhou_ssh_lib::core::{
    data_migration::{DataMigrationJournal, DataMigrationManifest, DataMigrationPhase},
    data_root::{
        migration_recovery_resolution, resolve_data_root, DataRootInputs, DataRootResolution,
        DataRootSource,
    },
    portable_root::{clear, load, save},
};

fn path(value: &str) -> Option<PathBuf> {
    Some(
        std::env::temp_dir()
            .join("qingzhou-data-root-tests")
            .join(value.replace([':', '\\'], "_")),
    )
}

#[test]
fn environment_override_wins_and_locks_the_root() {
    let resolved = resolve_data_root(DataRootInputs {
        env_override: path(r"D:\env-data"),
        portable_mode: true,
        portable_custom_root: path(r"E:\portable-custom"),
        portable_default_root: path(r"E:\app\data"),
        platform_root: path(r"F:\platform-data"),
    })
    .unwrap();
    assert_eq!(resolved.source, DataRootSource::Environment);
    assert_eq!(resolved.path, path(r"D:\env-data"));
    assert!(!resolved.mutable);
}

#[test]
fn portable_mode_uses_custom_then_default_and_never_platform_store() {
    let custom = resolve_data_root(DataRootInputs {
        portable_mode: true,
        portable_custom_root: path(r"E:\portable-custom"),
        portable_default_root: path(r"E:\app\data"),
        platform_root: path(r"F:\platform-data"),
        ..DataRootInputs::default()
    })
    .unwrap();
    assert_eq!(custom.source, DataRootSource::PortableCustom);
    assert_eq!(custom.path, path(r"E:\portable-custom"));
    assert!(custom.mutable);

    let default = resolve_data_root(DataRootInputs {
        portable_mode: true,
        portable_default_root: path(r"E:\app\data"),
        platform_root: path(r"F:\platform-data"),
        ..DataRootInputs::default()
    })
    .unwrap();
    assert_eq!(default.source, DataRootSource::PortableDefault);
    assert_eq!(default.path, path(r"E:\app\data"));
}

#[test]
fn installed_mode_uses_platform_store_or_requires_selection() {
    let platform = resolve_data_root(DataRootInputs {
        platform_root: path(r"F:\platform-data"),
        ..DataRootInputs::default()
    })
    .unwrap();
    assert_eq!(platform.source, DataRootSource::Platform);
    assert!(platform.mutable);

    let missing = resolve_data_root(DataRootInputs::default()).unwrap();
    assert_eq!(missing.source, DataRootSource::NeedsSelection);
    assert!(missing.path.is_none());
    assert!(missing.mutable);
}

#[test]
fn invalid_portable_pointer_is_rejected_instead_of_falling_back() {
    let error = resolve_data_root(DataRootInputs {
        portable_mode: true,
        portable_custom_root: Some(PathBuf::from("relative-data")),
        portable_default_root: path(r"E:\app\data"),
        platform_root: path(r"F:\platform-data"),
        ..DataRootInputs::default()
    })
    .unwrap_err();
    assert!(error.to_string().contains("绝对路径"));
}

#[test]
fn portable_pointer_round_trips_atomically_and_rejects_relative_paths() {
    let temp = tempfile::tempdir().unwrap();
    let pointer = temp.path().join("data-root.json");
    let root = temp.path().join("external-data");

    save(&pointer, &root).unwrap();
    assert_eq!(load(&pointer).unwrap(), Some(root));
    clear(&pointer).unwrap();
    assert_eq!(load(&pointer).unwrap(), None);
    assert!(save(&pointer, std::path::Path::new("relative-data")).is_err());
}

#[test]
fn invalid_migration_target_recovers_the_verified_old_root() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("old-root");
    let target = temp.path().join("new-root");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    let mut journal = DataMigrationJournal::prepared(
        source.clone(),
        target.clone(),
        DataRootSource::Platform,
        42,
        &DataMigrationManifest {
            entries: vec![],
            file_count: 0,
            total_bytes: 0,
        },
        1,
    );
    journal.phase = DataMigrationPhase::Failed;
    let current = DataRootResolution {
        source: DataRootSource::Platform,
        path: Some(target),
        mutable: true,
    };

    let recovered = migration_recovery_resolution(&current, &journal, false)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.path, Some(source));

    journal.phase = DataMigrationPhase::Completed;
    assert!(migration_recovery_resolution(&current, &journal, true)
        .unwrap()
        .is_none());
}
