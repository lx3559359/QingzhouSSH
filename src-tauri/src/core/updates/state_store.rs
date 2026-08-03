use std::{
    fs::{self, File},
    io::{self, BufReader, BufWriter, Write},
    path::{Component, Path, PathBuf},
    time::Duration,
};

use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::update::UpdateSource;

const MAX_STATE_BYTES: u64 = 64 * 1024;
const MAX_CLEANUP_DEPTH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredCheckStatus {
    UpToDate,
    Available,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredCheckResult {
    pub status: StoredCheckStatus,
    pub version: Option<String>,
    pub source: Option<UpdateSource>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdatePersistentState {
    pub auto_check: bool,
    pub last_checked_at: Option<u64>,
    pub last_result: Option<StoredCheckResult>,
    pub staged_file: Option<String>,
}

impl Default for UpdatePersistentState {
    fn default() -> Self {
        Self {
            auto_check: true,
            last_checked_at: None,
            last_result: None,
            staged_file: None,
        }
    }
}

impl UpdatePersistentState {
    pub fn automatic_check_due(&self, now: u64, interval: Duration) -> bool {
        if !self.auto_check {
            return false;
        }
        self.last_checked_at
            .map(|last| now.saturating_sub(last) >= interval.as_secs())
            .unwrap_or(true)
    }
}

#[derive(Debug, Error)]
pub enum UpdateStateStoreError {
    #[error("更新状态 I/O 失败：{0}")]
    Io(#[from] io::Error),
    #[error("更新状态格式无效：{0}")]
    Serialization(#[from] serde_json::Error),
    #[error("更新暂存路径无效：{0}")]
    InvalidStagedPath(String),
    #[error("更新状态原子替换失败：{0}")]
    AtomicWrite(String),
}

#[derive(Debug, Clone)]
pub struct UpdateStateStore {
    updates_dir: PathBuf,
    state_path: PathBuf,
}

impl UpdateStateStore {
    pub fn new(data_root: &Path) -> Result<Self, UpdateStateStoreError> {
        if !data_root.is_absolute() {
            return Err(UpdateStateStoreError::InvalidStagedPath(
                "数据根目录必须是绝对路径".into(),
            ));
        }
        let updates_dir = data_root.join("updates");
        fs::create_dir_all(&updates_dir)?;
        Ok(Self {
            state_path: updates_dir.join("state.json"),
            updates_dir,
        })
    }

    pub fn load(&self) -> Result<UpdatePersistentState, UpdateStateStoreError> {
        let file = match File::open(&self.state_path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(UpdatePersistentState::default());
            }
            Err(error) => return Err(error.into()),
        };
        if file.metadata()?.len() > MAX_STATE_BYTES {
            return Err(UpdateStateStoreError::InvalidStagedPath(
                "状态文件超过大小上限".into(),
            ));
        }
        let state: UpdatePersistentState = serde_json::from_reader(BufReader::new(file))?;
        validate_staged_file(state.staged_file.as_deref())?;
        Ok(state)
    }

    pub fn save(&self, state: &UpdatePersistentState) -> Result<(), UpdateStateStoreError> {
        validate_staged_file(state.staged_file.as_deref())?;
        let payload = serde_json::to_vec_pretty(state)?;
        if payload.len() as u64 > MAX_STATE_BYTES {
            return Err(UpdateStateStoreError::InvalidStagedPath(
                "状态文件超过大小上限".into(),
            ));
        }
        AtomicFile::new(&self.state_path, AllowOverwrite)
            .write(|file| {
                let mut writer = BufWriter::new(file);
                writer.write_all(&payload)?;
                writer.flush()?;
                writer.get_ref().sync_all()
            })
            .map_err(|error| UpdateStateStoreError::AtomicWrite(error.to_string()))
    }

    pub fn cleanup_partial_files(&self) -> Result<usize, UpdateStateStoreError> {
        cleanup_directory(&self.updates_dir, 0).map_err(Into::into)
    }

    pub fn resolve_staged_file(&self, relative: &str) -> Result<PathBuf, UpdateStateStoreError> {
        validate_staged_file(Some(relative))?;
        Ok(self.updates_dir.join(relative))
    }

    pub fn updates_dir(&self) -> &Path {
        &self.updates_dir
    }
}

fn validate_staged_file(value: Option<&str>) -> Result<(), UpdateStateStoreError> {
    let Some(value) = value else {
        return Ok(());
    };
    let path = Path::new(value);
    let components: Vec<_> = path.components().collect();
    let safe = !components.is_empty()
        && components.len() <= 4
        && matches!(components.first(), Some(Component::Normal(name)) if *name == "staged")
        && components
            .iter()
            .all(|component| matches!(component, Component::Normal(_)));
    if !safe {
        return Err(UpdateStateStoreError::InvalidStagedPath(value.into()));
    }
    Ok(())
}

fn cleanup_directory(directory: &Path, depth: usize) -> io::Result<usize> {
    if depth > MAX_CLEANUP_DEPTH || !directory.exists() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            removed += cleanup_directory(&path, depth + 1)?;
        } else if file_type.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".partial"))
        {
            fs::remove_file(path)?;
            removed += 1;
        }
    }
    Ok(removed)
}
