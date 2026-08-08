use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

const POINTER_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlatformDataRootPointer {
    schema_version: u32,
    data_root: PathBuf,
}

pub trait DataRootStore: Send + Sync {
    fn load(&self) -> AppResult<Option<PathBuf>>;
    fn save(&self, path: &Path) -> AppResult<()>;
    fn clear(&self) -> AppResult<()>;
}

#[derive(Debug, Clone)]
pub struct FileDataRootStore {
    pointer_path: PathBuf,
}

impl FileDataRootStore {
    pub fn new(pointer_path: PathBuf) -> Self {
        Self { pointer_path }
    }

    #[cfg(test)]
    fn pointer_path(&self) -> &Path {
        &self.pointer_path
    }
}

impl DataRootStore for FileDataRootStore {
    fn load(&self) -> AppResult<Option<PathBuf>> {
        let payload = match fs::read(&self.pointer_path) {
            Ok(payload) => payload,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let pointer: PlatformDataRootPointer = serde_json::from_slice(&payload)
            .map_err(|error| AppError::Serialization(error.to_string()))?;
        if pointer.schema_version != POINTER_SCHEMA_VERSION || !pointer.data_root.is_absolute() {
            return Err(AppError::Validation("平台数据目录指针无效".into()));
        }
        Ok(Some(pointer.data_root))
    }

    fn save(&self, path: &Path) -> AppResult<()> {
        if !path.is_absolute() {
            return Err(AppError::Validation("数据目录必须是绝对路径".into()));
        }
        let parent = self
            .pointer_path
            .parent()
            .ok_or_else(|| AppError::Validation("平台数据目录指针路径无效".into()))?;
        fs::create_dir_all(parent)?;
        let payload = serde_json::to_vec_pretty(&PlatformDataRootPointer {
            schema_version: POINTER_SCHEMA_VERSION,
            data_root: path.to_path_buf(),
        })
        .map_err(|error| AppError::Serialization(error.to_string()))?;
        AtomicFile::new(&self.pointer_path, AllowOverwrite)
            .write(|file| {
                let mut writer = BufWriter::new(file);
                writer.write_all(&payload)?;
                writer.flush()?;
                writer.get_ref().sync_all()
            })
            .map_err(|error| AppError::Io(std::io::Error::other(error.to_string())))
    }

    fn clear(&self) -> AppResult<()> {
        match fs::remove_file(&self.pointer_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy, Default)]
struct WindowsRegistryDataRootStore;

#[cfg(windows)]
impl DataRootStore for WindowsRegistryDataRootStore {
    fn load(&self) -> AppResult<Option<PathBuf>> {
        crate::core::root_registry::load_data_root()
    }

    fn save(&self, path: &Path) -> AppResult<()> {
        crate::core::root_registry::save_data_root(path)
    }

    fn clear(&self) -> AppResult<()> {
        crate::core::root_registry::clear_data_root()
    }
}

pub fn system_data_root_store() -> AppResult<Box<dyn DataRootStore>> {
    #[cfg(windows)]
    {
        Ok(Box::new(WindowsRegistryDataRootStore))
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        Ok(Box::new(FileDataRootStore::new(platform_pointer_path()?)))
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Err(AppError::Compatibility(
            "当前客户端平台尚未提供数据目录存储实现".into(),
        ))
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn platform_pointer_path() -> AppResult<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    {
        platform_pointer_path_for("macos", None, home)
    }
    #[cfg(target_os = "linux")]
    {
        platform_pointer_path_for(
            "linux",
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            home,
        )
    }
}

#[cfg(any(test, target_os = "macos", target_os = "linux"))]
fn platform_pointer_path_for(
    platform: &str,
    xdg_config_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> AppResult<PathBuf> {
    match platform {
        "macos" => home
            .map(|path| {
                path.join("Library/Application Support/com.qingzhoussh.desktop/data-root.json")
            })
            .ok_or_else(|| AppError::Compatibility("macOS 用户目录不可用".into())),
        "linux" => xdg_config_home
            .or_else(|| home.map(|path| path.join(".config")))
            .map(|path| path.join("qingzhou-ssh/data-root.json"))
            .ok_or_else(|| AppError::Compatibility("Linux 配置目录不可用".into())),
        _ => Err(AppError::Compatibility("未知客户端平台".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_round_trips_and_clears_an_absolute_root() {
        let temp = tempfile::tempdir().unwrap();
        let store = FileDataRootStore::new(temp.path().join("config/data-root.json"));
        let root = temp.path().join("data");

        assert_eq!(store.load().unwrap(), None);
        store.save(&root).unwrap();
        assert_eq!(store.load().unwrap(), Some(root));
        assert!(store.pointer_path().is_file());
        store.clear().unwrap();
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn platform_file_locations_follow_macos_and_xdg_conventions() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            platform_pointer_path_for("macos", None, Some(temp.path().into())).unwrap(),
            temp.path()
                .join("Library/Application Support/com.qingzhoussh.desktop/data-root.json")
        );
        let xdg = temp.path().join("xdg");
        assert_eq!(
            platform_pointer_path_for("linux", Some(xdg.clone()), None).unwrap(),
            xdg.join("qingzhou-ssh/data-root.json")
        );
        assert_eq!(
            platform_pointer_path_for("linux", None, Some(temp.path().into())).unwrap(),
            temp.path().join(".config/qingzhou-ssh/data-root.json")
        );
    }
}
