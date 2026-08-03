use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    core::sftp::validate_remote_path,
    error::{AppError, AppResult},
};

const MAX_BACKUP_NAME_BYTES: usize = 96;

pub fn restore_point_relative_path(
    run_id: Uuid,
    node_id: Uuid,
    remote_path: &str,
) -> AppResult<String> {
    validate_remote_path(remote_path)?;
    let original_name = remote_path
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| AppError::Validation("恢复点远程路径必须指向文件".into()))?;
    let mut safe_name = original_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    while safe_name.len() > MAX_BACKUP_NAME_BYTES {
        safe_name.pop();
    }
    if safe_name.is_empty() || safe_name == "." || safe_name == ".." {
        safe_name = "remote-file".into();
    }
    let digest = format!("{:x}", Sha256::digest(remote_path.as_bytes()));
    let relative = format!(
        "backups/workflows/{run_id}/{node_id}/{safe_name}.{}.backup",
        &digest[..12]
    );
    validate_restore_point_relative_path(&relative)?;
    Ok(relative)
}

pub fn validate_restore_point_relative_path(relative_path: &str) -> AppResult<()> {
    if relative_path.is_empty()
        || relative_path.contains('\0')
        || relative_path.contains('\\')
        || Path::new(relative_path).is_absolute()
    {
        return Err(AppError::Security(
            "恢复点路径不是安全的项目内相对路径".into(),
        ));
    }
    let components = Path::new(relative_path).components().collect::<Vec<_>>();
    if components.len() != 5
        || components[0] != Component::Normal("backups".as_ref())
        || components[1] != Component::Normal("workflows".as_ref())
    {
        return Err(AppError::Security(
            "恢复点只能位于 backups/workflows/<run>/<node>".into(),
        ));
    }
    let Component::Normal(run) = components[2] else {
        return Err(AppError::Security("恢复点运行目录无效".into()));
    };
    let Component::Normal(node) = components[3] else {
        return Err(AppError::Security("恢复点节点目录无效".into()));
    };
    let Component::Normal(file_name) = components[4] else {
        return Err(AppError::Security("恢复点文件名无效".into()));
    };
    Uuid::parse_str(&run.to_string_lossy())
        .map_err(|_| AppError::Security("恢复点运行目录无效".into()))?;
    Uuid::parse_str(&node.to_string_lossy())
        .map_err(|_| AppError::Security("恢复点节点目录无效".into()))?;
    let file_name = file_name.to_string_lossy();
    if file_name.is_empty()
        || file_name == "."
        || file_name == ".."
        || file_name.contains(':')
        || file_name.len() > 160
    {
        return Err(AppError::Security("恢复点文件名无效".into()));
    }
    Ok(())
}

pub fn resolve_restore_point_path(data_root: &Path, relative_path: &str) -> AppResult<PathBuf> {
    if !data_root.is_absolute() {
        return Err(AppError::Security("数据根目录必须是绝对路径".into()));
    }
    validate_restore_point_relative_path(relative_path)?;
    Ok(data_root.join(relative_path))
}
