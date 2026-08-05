use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
};
use uuid::Uuid;

use crate::{
    core::tasks::ValidatedParameters,
    error::{AppError, AppResult},
};

const MAX_ASSET_NAME_BYTES: usize = 96;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRestoreAsset {
    pub relative_path: String,
    pub bytes: u64,
    pub sha256: String,
}

pub fn task_restore_dir(run_id: Uuid) -> PathBuf {
    PathBuf::from(format!("backups/tasks/{run_id}"))
}

pub fn task_restore_item_relative_path(
    run_id: Uuid,
    ordinal: usize,
    remote_target: &str,
) -> AppResult<PathBuf> {
    if remote_target.is_empty() || remote_target.contains('\0') || ordinal >= 1000 {
        return Err(AppError::Validation("恢复项目标或序号无效".into()));
    }
    let original_name = remote_target
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("remote-state");
    let safe_name = safe_asset_name(original_name);
    let digest = format!("{:x}", Sha256::digest(remote_target.as_bytes()));
    let relative = PathBuf::from(format!(
        "backups/tasks/{run_id}/{ordinal:03}-{safe_name}.{}.backup",
        &digest[..12]
    ));
    validate_restore_relative_path(&relative)?;
    Ok(relative)
}

pub fn validate_restore_relative_path(relative: &Path) -> AppResult<()> {
    validate_confined_relative_path(relative)?;
    let components = relative.components().collect::<Vec<_>>();
    if !(components.len() == 3 || components.len() == 4)
        || components[0] != Component::Normal("backups".as_ref())
        || components[1] != Component::Normal("tasks".as_ref())
    {
        return Err(AppError::Security(
            "任务恢复资产只能位于 backups/tasks/<run_uuid>".into(),
        ));
    }
    let Component::Normal(run_id) = components[2] else {
        return Err(AppError::Security("任务恢复目录无效".into()));
    };
    Uuid::parse_str(&run_id.to_string_lossy())
        .map_err(|_| AppError::Security("任务恢复目录必须使用运行 UUID".into()))?;
    if components.len() == 4 {
        let Component::Normal(file_name) = components[3] else {
            return Err(AppError::Security("任务恢复文件名无效".into()));
        };
        let file_name = file_name.to_string_lossy();
        if file_name.is_empty()
            || file_name.len() > 180
            || file_name.ends_with(".partial")
            || file_name.contains(':')
        {
            return Err(AppError::Security("任务恢复文件名无效".into()));
        }
    }
    Ok(())
}

pub fn resolve_task_restore_path(data_root: &Path, relative: &Path) -> AppResult<PathBuf> {
    if !data_root.is_absolute() {
        return Err(AppError::Security("数据根目录必须是绝对路径".into()));
    }
    validate_restore_relative_path(relative)?;
    Ok(data_root.join(relative))
}

pub async fn write_restore_asset_atomic(
    data_root: &Path,
    relative: &Path,
    contents: &[u8],
) -> AppResult<StoredRestoreAsset> {
    let destination = prepare_task_restore_destination(data_root, relative).await?;
    let parent = destination
        .parent()
        .ok_or_else(|| AppError::Security("恢复资产目录无效".into()))?;

    let file_name = destination
        .file_name()
        .ok_or_else(|| AppError::Security("恢复资产文件名无效".into()))?
        .to_string_lossy();
    let partial = parent.join(format!(".{file_name}.{}.partial", Uuid::new_v4()));
    let outcome = async {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&partial)
            .await?;
        file.write_all(contents).await?;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        fs::rename(&partial, &destination).await?;
        let metadata = fs::symlink_metadata(&destination).await?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(AppError::Security("恢复资产不是普通文件".into()));
        }
        Ok(StoredRestoreAsset {
            relative_path: relative.to_string_lossy().replace('\\', "/"),
            bytes: metadata.len(),
            sha256: format!("{:x}", Sha256::digest(contents)),
        })
    }
    .await;
    if outcome.is_err() {
        let _ = fs::remove_file(&partial).await;
    }
    outcome
}

pub(crate) async fn prepare_task_restore_destination(
    data_root: &Path,
    relative: &Path,
) -> AppResult<PathBuf> {
    validate_restore_relative_path(relative)?;
    if relative.components().count() != 4 {
        return Err(AppError::Security("恢复资产必须指向任务恢复文件".into()));
    }
    let destination = resolve_task_restore_path(data_root, relative)?;
    let parent = destination
        .parent()
        .ok_or_else(|| AppError::Security("恢复资产目录无效".into()))?;
    ensure_safe_directory_chain(data_root, parent).await?;
    if fs::symlink_metadata(&destination).await.is_ok() {
        return Err(AppError::Security("恢复资产目标已经存在".into()));
    }
    Ok(destination)
}

pub(crate) fn render_backup_target(
    template: &str,
    implementation_id: &str,
    parameters: &ValidatedParameters,
) -> AppResult<String> {
    if template.is_empty() || template.contains('\0') {
        return Err(AppError::Validation(format!(
            "任务实现 {implementation_id} 的恢复目标无效"
        )));
    }
    let mut rendered = template.to_owned();
    for (name, parameter) in parameters.iter() {
        let value = match &parameter.value {
            serde_json::Value::String(value) => value.clone(),
            serde_json::Value::Number(value) => value.to_string(),
            serde_json::Value::Bool(value) => value.to_string(),
            _ => {
                return Err(AppError::Validation(format!(
                    "恢复目标参数 {name} 不是单值"
                )))
            }
        };
        rendered = rendered.replace(&format!("{{{{{name}}}}}"), &value);
    }
    if rendered.contains("{{") || rendered.contains("}}") || rendered.contains('\0') {
        return Err(AppError::Validation(format!(
            "任务实现 {implementation_id} 包含未解析的恢复目标"
        )));
    }
    Ok(rendered)
}

pub(crate) fn validate_confined_relative_path(relative: &Path) -> AppResult<()> {
    let text = relative.to_string_lossy();
    if text.is_empty()
        || text.contains('\0')
        || text.contains('\\')
        || relative.is_absolute()
        || relative.components().any(|component| {
            !matches!(component, Component::Normal(_))
                || component.as_os_str().to_string_lossy().contains(':')
        })
    {
        return Err(AppError::Security(
            "恢复资产路径不是安全的项目内相对路径".into(),
        ));
    }
    Ok(())
}

async fn ensure_safe_directory_chain(data_root: &Path, directory: &Path) -> AppResult<()> {
    if !data_root.is_absolute() || !directory.starts_with(data_root) {
        return Err(AppError::Security("恢复资产目录逃逸数据根目录".into()));
    }
    let root_metadata = fs::symlink_metadata(data_root).await?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(AppError::Security("数据根目录不是安全的普通目录".into()));
    }
    let relative = directory
        .strip_prefix(data_root)
        .map_err(|_| AppError::Security("恢复资产目录逃逸数据根目录".into()))?;
    let mut current = data_root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(AppError::Security("恢复资产目录包含无效路径段".into()));
        };
        current.push(component);
        match fs::symlink_metadata(&current).await {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(AppError::Security("恢复资产目录包含符号链接".into()));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).await?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn safe_asset_name(value: &str) -> String {
    let mut name = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    while name.len() > MAX_ASSET_NAME_BYTES {
        name.pop();
    }
    if name.is_empty() || name == "." || name == ".." {
        "remote-state".into()
    } else {
        name
    }
}
