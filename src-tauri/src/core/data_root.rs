use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Default)]
pub struct DataRootInputs {
    pub env_override: Option<PathBuf>,
    pub portable_mode: bool,
    pub portable_custom_root: Option<PathBuf>,
    pub portable_default_root: Option<PathBuf>,
    pub platform_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataRootSource {
    Environment,
    PortableCustom,
    PortableDefault,
    Platform,
    Registry,
    NeedsSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DataRootResolution {
    pub source: DataRootSource,
    pub path: Option<PathBuf>,
    pub mutable: bool,
}

pub fn resolve_data_root(input: DataRootInputs) -> AppResult<DataRootResolution> {
    if let Some(path) = input.env_override {
        validate_root_path(&path)?;
        return Ok(DataRootResolution {
            source: DataRootSource::Environment,
            path: Some(path),
            mutable: false,
        });
    }
    if input.portable_mode {
        if let Some(path) = input.portable_custom_root {
            validate_root_path(&path)?;
            return Ok(DataRootResolution {
                source: DataRootSource::PortableCustom,
                path: Some(path),
                mutable: true,
            });
        }
        let path = input
            .portable_default_root
            .ok_or_else(|| AppError::Validation("便携版默认数据目录不可用".into()))?;
        validate_root_path(&path)?;
        return Ok(DataRootResolution {
            source: DataRootSource::PortableDefault,
            path: Some(path),
            mutable: true,
        });
    }
    if let Some(path) = input.platform_root {
        validate_root_path(&path)?;
        return Ok(DataRootResolution {
            source: DataRootSource::Platform,
            path: Some(path),
            mutable: true,
        });
    }
    Ok(DataRootResolution {
        source: DataRootSource::NeedsSelection,
        path: None,
        mutable: true,
    })
}

fn validate_root_path(path: &Path) -> AppResult<()> {
    if !path.is_absolute() {
        return Err(AppError::Validation("数据目录必须是绝对路径".into()));
    }
    Ok(())
}

pub fn initialize_data_root(root: &Path) -> AppResult<()> {
    if !root.is_absolute() {
        return Err(AppError::Validation("数据目录必须是绝对路径".into()));
    }

    fs::create_dir_all(root)?;
    for name in [
        "vault",
        "logs",
        "downloads",
        "backups",
        "templates",
        "cache",
        "updates",
    ] {
        fs::create_dir_all(root.join(name))?;
    }

    let probe = root.join(format!(".write-probe-{}", uuid::Uuid::new_v4()));
    let probe_result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)?;
        file.write_all(b"qingzhou")?;
        file.sync_all()
    })();
    let cleanup_result = fs::remove_file(&probe);
    probe_result?;
    cleanup_result?;
    Ok(())
}

pub fn resolve_runtime_data_root() -> AppResult<DataRootResolution> {
    let executable = std::env::current_exe()?;
    let executable_directory = executable
        .parent()
        .ok_or_else(|| AppError::Validation("无法确定程序目录".into()))?;
    let portable_mode = executable_directory.join("portable.flag").is_file();
    let portable_custom_root = if portable_mode {
        crate::core::portable_root::load(&executable_directory.join("data-root.json"))?
    } else {
        None
    };

    let resolution = resolve_data_root(DataRootInputs {
        env_override: std::env::var_os("QINGZHOU_DATA_ROOT").map(PathBuf::from),
        portable_mode,
        portable_custom_root,
        portable_default_root: portable_mode.then(|| executable_directory.join("data")),
        platform_root: if portable_mode {
            None
        } else {
            crate::core::data_root_store::system_data_root_store()?.load()?
        },
    })?;
    recover_runtime_resolution(resolution, executable_directory)
}

pub fn migration_recovery_resolution(
    current: &DataRootResolution,
    journal: &crate::core::data_migration::DataMigrationJournal,
    completion_marker_valid: bool,
) -> AppResult<Option<DataRootResolution>> {
    let Some(current_root) = current.path.as_deref() else {
        return Ok(None);
    };
    if current.source == DataRootSource::Environment
        || !same_runtime_path(current_root, &journal.target)
    {
        return Ok(None);
    }
    if journal.phase == crate::core::data_migration::DataMigrationPhase::Completed
        && completion_marker_valid
    {
        return Ok(None);
    }
    if !journal.source.is_absolute()
        || !journal.source.is_dir()
        || same_runtime_path(&journal.source, &journal.target)
        || matches!(
            journal.source_mode,
            DataRootSource::Environment | DataRootSource::NeedsSelection
        )
    {
        return Err(AppError::Security(
            "迁移目标状态无效，且无法验证旧数据目录".into(),
        ));
    }
    Ok(Some(DataRootResolution {
        source: journal.source_mode,
        path: Some(journal.source.clone()),
        mutable: true,
    }))
}

fn same_runtime_path(left: &Path, right: &Path) -> bool {
    let left = fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn recover_runtime_resolution(
    resolution: DataRootResolution,
    executable_directory: &Path,
) -> AppResult<DataRootResolution> {
    let Some(root) = resolution.path.as_deref() else {
        return Ok(resolution);
    };
    let journal_path = root.join(crate::core::data_migration::MIGRATION_JOURNAL_FILE);
    if !journal_path.is_file() {
        return Ok(resolution);
    }
    let journal = crate::core::data_migration::MigrationJournalStore::load(&journal_path)?;
    let marker_valid = crate::core::data_migration::completion_marker_is_valid(root, &journal);
    let Some(recovered) = migration_recovery_resolution(&resolution, &journal, marker_valid)?
    else {
        return Ok(resolution);
    };
    match recovered.source {
        DataRootSource::Platform | DataRootSource::Registry => {
            crate::core::data_root_store::system_data_root_store()?
                .save(recovered.path.as_deref().unwrap())?
        }
        DataRootSource::PortableDefault => {
            crate::core::portable_root::clear(&executable_directory.join("data-root.json"))?
        }
        DataRootSource::PortableCustom => crate::core::portable_root::save(
            &executable_directory.join("data-root.json"),
            recovered.path.as_deref().unwrap(),
        )?,
        DataRootSource::Environment | DataRootSource::NeedsSelection => unreachable!(),
    }
    Ok(recovered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn environment_override_wins_over_portable_and_registry() {
        let temp = tempdir().unwrap();
        let environment_root = temp.path().join("work-data");
        let input = DataRootInputs {
            env_override: Some(environment_root.clone()),
            portable_mode: true,
            portable_custom_root: Some(temp.path().join("app-custom")),
            portable_default_root: Some(temp.path().join("app-data")),
            platform_root: Some(temp.path().join("saved")),
        };
        let resolved = resolve_data_root(input).unwrap();
        assert_eq!(resolved.source, DataRootSource::Environment);
        assert_eq!(resolved.path.unwrap(), environment_root);
    }

    #[test]
    fn no_source_requires_user_selection_instead_of_appdata_fallback() {
        let resolved = resolve_data_root(DataRootInputs::default()).unwrap();
        assert_eq!(resolved.source, DataRootSource::NeedsSelection);
        assert!(resolved.path.is_none());
    }

    #[test]
    fn initialization_creates_only_declared_subdirectories() {
        let temp = tempdir().unwrap();
        initialize_data_root(temp.path()).unwrap();
        for name in [
            "vault",
            "logs",
            "downloads",
            "backups",
            "templates",
            "cache",
            "updates",
        ] {
            assert!(temp.path().join(name).is_dir(), "missing {name}");
        }
        assert!(!temp.path().join("AppData").exists());
    }

    #[test]
    fn initialization_preserves_an_existing_probe_named_file() {
        let temp = tempdir().unwrap();
        let existing_probe = temp.path().join(".write-probe");
        std::fs::write(&existing_probe, b"keep me").unwrap();

        initialize_data_root(temp.path()).unwrap();

        assert_eq!(std::fs::read(existing_probe).unwrap(), b"keep me");
    }

    #[cfg(windows)]
    #[test]
    fn runtime_path_comparison_matches_windows_case_insensitively() {
        assert!(same_runtime_path(
            Path::new(r"D:\Qingzhou\Data"),
            Path::new(r"d:\qingzhou\data")
        ));
    }
}
