use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::error::{AppError, AppResult};

#[derive(Debug, Default)]
pub struct DataRootInputs {
    pub env_override: Option<PathBuf>,
    pub portable_root: Option<PathBuf>,
    pub registry_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DataRootSource {
    Environment,
    Portable,
    Registry,
    NeedsSelection,
}

#[derive(Debug, Serialize)]
pub struct DataRootResolution {
    pub source: DataRootSource,
    pub path: Option<PathBuf>,
}

pub fn resolve_data_root(input: DataRootInputs) -> DataRootResolution {
    if let Some(path) = input.env_override {
        return DataRootResolution {
            source: DataRootSource::Environment,
            path: Some(path),
        };
    }
    if let Some(path) = input.portable_root {
        return DataRootResolution {
            source: DataRootSource::Portable,
            path: Some(path),
        };
    }
    if let Some(path) = input.registry_root {
        return DataRootResolution {
            source: DataRootSource::Registry,
            path: Some(path),
        };
    }
    DataRootResolution {
        source: DataRootSource::NeedsSelection,
        path: None,
    }
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
    let portable_root = executable_directory
        .join("portable.flag")
        .is_file()
        .then(|| executable_directory.join("data"));

    Ok(resolve_data_root(DataRootInputs {
        env_override: std::env::var_os("QINGZHOU_DATA_ROOT").map(PathBuf::from),
        portable_root,
        registry_root: crate::core::root_registry::load_data_root()?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn environment_override_wins_over_portable_and_registry() {
        let input = DataRootInputs {
            env_override: Some(r"D:\work\data".into()),
            portable_root: Some(r"D:\app\data".into()),
            registry_root: Some(r"E:\saved".into()),
        };
        let resolved = resolve_data_root(input);
        assert_eq!(resolved.source, DataRootSource::Environment);
        assert_eq!(
            resolved.path.unwrap(),
            std::path::PathBuf::from(r"D:\work\data")
        );
    }

    #[test]
    fn no_source_requires_user_selection_instead_of_appdata_fallback() {
        let resolved = resolve_data_root(DataRootInputs::default());
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
}
