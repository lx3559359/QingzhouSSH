use std::path::{Path, PathBuf};

use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};

use crate::{
    core::ssh::transport::AuthenticatedSshSession,
    error::{AppError, AppResult},
};

use super::validate_remote_path;

const MAX_DIRECTORY_ENTRIES: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserEntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserEntry {
    pub name: String,
    pub path: String,
    pub kind: BrowserEntryKind,
    pub size: Option<u64>,
    pub modified_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryListing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<BrowserEntry>,
}

pub fn validate_remote_directory_path(path: &str) -> AppResult<()> {
    validate_remote_path(path)?;
    let windows_drive_root = {
        let bytes = path.as_bytes();
        bytes.len() == 3 && bytes[0].is_ascii_alphabetic() && &bytes[1..] == b":/"
    };
    if path != "/" && !windows_drive_root && path.ends_with('/') {
        return Err(AppError::Validation("远程目录末尾不需要重复的斜杠".into()));
    }
    Ok(())
}

pub fn remote_parent(path: &str) -> Option<String> {
    if path == "/" {
        return None;
    }
    let trimmed = path.trim_end_matches('/');
    let parent = trimmed.rsplit_once('/').map(|(value, _)| value)?;
    let parent = if parent.is_empty() {
        "/".to_string()
    } else if parent.len() == 2 && parent.ends_with(':') {
        format!("{parent}/")
    } else {
        parent.to_string()
    };
    Some(parent)
}

pub fn validate_remote_entry_name(name: &str) -> AppResult<()> {
    if name.trim().is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
        || name.len() > 255
    {
        return Err(AppError::Validation(
            "远程名称必须是 1–255 字节且不含路径分隔符、NUL、. 或 ..".into(),
        ));
    }
    Ok(())
}

pub fn remote_child_path(directory: &str, name: &str) -> AppResult<String> {
    validate_remote_directory_path(directory)?;
    validate_remote_entry_name(name)?;
    Ok(if directory.ends_with('/') {
        format!("{directory}{name}")
    } else {
        format!("{directory}/{name}")
    })
}

pub async fn list_remote_directory(
    ssh: &AuthenticatedSshSession,
    path: &str,
) -> AppResult<DirectoryListing> {
    validate_remote_directory_path(path)?;
    let sftp = open_sftp(ssh).await?;
    let result = async {
        let read_dir = sftp
            .read_dir(path)
            .await
            .map_err(|error| AppError::Transfer(format!("无法读取远程目录 {path}：{error}")))?;
        let mut entries = Vec::new();
        for entry in read_dir.take(MAX_DIRECTORY_ENTRIES) {
            let name = entry.file_name();
            if name.is_empty() || name.contains('/') || name.contains('\0') {
                continue;
            }
            let metadata = entry.metadata();
            let kind = if metadata.is_dir() {
                BrowserEntryKind::Directory
            } else if metadata.is_regular() {
                BrowserEntryKind::File
            } else if metadata.is_symlink() {
                BrowserEntryKind::Symlink
            } else {
                BrowserEntryKind::Other
            };
            entries.push(BrowserEntry {
                name,
                path: entry.path(),
                kind,
                size: metadata.is_regular().then_some(metadata.size).flatten(),
                modified_at: metadata.mtime.map(u64::from),
            });
        }
        sort_entries(&mut entries);
        Ok(DirectoryListing {
            path: path.to_string(),
            parent: remote_parent(path),
            entries,
        })
    }
    .await;
    close_sftp(sftp, result).await
}

pub async fn create_remote_directory(
    ssh: &AuthenticatedSshSession,
    parent: &str,
    name: &str,
) -> AppResult<()> {
    let target = remote_child_path(parent, name)?;
    let sftp = open_sftp(ssh).await?;
    let result = async {
        if sftp
            .try_exists(target.clone())
            .await
            .map_err(|error| sftp_operation_error("检查远程目录", &target, error))?
        {
            return Err(AppError::Validation(format!(
                "远程对象已存在，未创建：{target}"
            )));
        }
        sftp.create_dir(target.clone())
            .await
            .map_err(|error| sftp_operation_error("创建远程目录", &target, error))
    }
    .await;
    close_sftp(sftp, result).await
}

pub async fn rename_remote_entry(
    ssh: &AuthenticatedSshSession,
    path: &str,
    new_name: &str,
) -> AppResult<()> {
    validate_remote_path(path)?;
    let parent =
        remote_parent(path).ok_or_else(|| AppError::Validation("不能重命名远程根目录".into()))?;
    let target = remote_child_path(&parent, new_name)?;
    if target == path {
        return Ok(());
    }

    let sftp = open_sftp(ssh).await?;
    let result = async {
        let metadata = sftp
            .symlink_metadata(path.to_string())
            .await
            .map_err(|error| sftp_operation_error("读取远程对象", path, error))?;
        if metadata.is_symlink() || !(metadata.is_dir() || metadata.is_regular()) {
            return Err(AppError::Validation(
                "只允许重命名普通文件或文件夹；符号链接和特殊对象不会被修改".into(),
            ));
        }
        if sftp
            .try_exists(target.clone())
            .await
            .map_err(|error| sftp_operation_error("检查重命名目标", &target, error))?
        {
            return Err(AppError::Validation(format!(
                "同名远程对象已存在，未重命名：{target}"
            )));
        }
        sftp.rename(path.to_string(), target.clone())
            .await
            .map_err(|error| sftp_operation_error("重命名远程对象", path, error))
    }
    .await;
    close_sftp(sftp, result).await
}

pub async fn delete_remote_entry(
    ssh: &AuthenticatedSshSession,
    path: &str,
    expected_kind: BrowserEntryKind,
) -> AppResult<()> {
    validate_remote_path(path)?;
    if remote_parent(path).is_none() {
        return Err(AppError::Validation("不能删除远程根目录".into()));
    }
    if !matches!(
        expected_kind,
        BrowserEntryKind::File | BrowserEntryKind::Directory
    ) {
        return Err(AppError::Validation("只允许删除普通文件或空文件夹".into()));
    }

    let sftp = open_sftp(ssh).await?;
    let result = async {
        let metadata = sftp
            .symlink_metadata(path.to_string())
            .await
            .map_err(|error| sftp_operation_error("读取待删除远程对象", path, error))?;
        if metadata.is_symlink() {
            return Err(AppError::Validation(
                "符号链接不会通过文件浏览器删除".into(),
            ));
        }
        match expected_kind {
            BrowserEntryKind::File if metadata.is_regular() => sftp
                .remove_file(path.to_string())
                .await
                .map_err(|error| sftp_operation_error("删除远程文件", path, error)),
            BrowserEntryKind::Directory if metadata.is_dir() => {
                sftp.remove_dir(path.to_string()).await.map_err(|error| {
                    AppError::Transfer(format!(
                        "删除远程文件夹失败（仅允许删除空文件夹） {path}：{error}"
                    ))
                })
            }
            _ => Err(AppError::Validation(
                "远程对象类型已变化，已停止删除并刷新后重试".into(),
            )),
        }
    }
    .await;
    close_sftp(sftp, result).await
}

pub async fn list_local_directory(
    data_root: &Path,
    path: Option<&Path>,
) -> AppResult<DirectoryListing> {
    let directory = path.unwrap_or(data_root);
    if !directory.is_absolute() {
        return Err(AppError::Validation("本地目录必须是绝对路径".into()));
    }
    let metadata = tokio::fs::metadata(directory).await?;
    if !metadata.is_dir() {
        return Err(AppError::Validation("所选本地路径不是目录".into()));
    }

    let mut read_dir = tokio::fs::read_dir(directory).await?;
    let mut entries = Vec::new();
    while entries.len() < MAX_DIRECTORY_ENTRIES {
        let Some(entry) = read_dir.next_entry().await? else {
            break;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.is_empty() || name.contains('\0') {
            continue;
        }
        let metadata = entry.metadata().await?;
        let file_type = metadata.file_type();
        let kind = if file_type.is_dir() {
            BrowserEntryKind::Directory
        } else if file_type.is_file() {
            BrowserEntryKind::File
        } else if file_type.is_symlink() {
            BrowserEntryKind::Symlink
        } else {
            BrowserEntryKind::Other
        };
        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|value| value.as_secs());
        entries.push(BrowserEntry {
            name,
            path: entry.path().to_string_lossy().into_owned(),
            kind,
            size: file_type.is_file().then_some(metadata.len()),
            modified_at,
        });
    }
    sort_entries(&mut entries);
    Ok(DirectoryListing {
        path: directory.to_string_lossy().into_owned(),
        parent: directory
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(PathBuf::from)
            .map(|parent| parent.to_string_lossy().into_owned()),
        entries,
    })
}

fn sort_entries(entries: &mut [BrowserEntry]) {
    entries.sort_by(|left, right| {
        entry_rank(left.kind)
            .cmp(&entry_rank(right.kind))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
}

fn entry_rank(kind: BrowserEntryKind) -> u8 {
    match kind {
        BrowserEntryKind::Directory => 0,
        BrowserEntryKind::File => 1,
        BrowserEntryKind::Symlink => 2,
        BrowserEntryKind::Other => 3,
    }
}

async fn open_sftp(ssh: &AuthenticatedSshSession) -> AppResult<SftpSession> {
    let channel = ssh.open_session_channel().await?;
    channel.request_subsystem(true, "sftp").await?;
    SftpSession::new(channel.into_stream())
        .await
        .map_err(|error| AppError::Transfer(error.to_string()))
}

fn sftp_operation_error(
    operation: &str,
    path: &str,
    error: russh_sftp::client::error::Error,
) -> AppError {
    AppError::Transfer(format!("{operation}失败 {path}：{error}"))
}

async fn close_sftp<T>(sftp: SftpSession, result: AppResult<T>) -> AppResult<T> {
    let _ = sftp.close().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_directories_before_files_without_case_bias() {
        let mut entries = vec![
            BrowserEntry {
                name: "z.log".into(),
                path: "/z.log".into(),
                kind: BrowserEntryKind::File,
                size: Some(1),
                modified_at: None,
            },
            BrowserEntry {
                name: "Alpha".into(),
                path: "/Alpha".into(),
                kind: BrowserEntryKind::Directory,
                size: None,
                modified_at: None,
            },
        ];
        sort_entries(&mut entries);
        assert_eq!(entries[0].name, "Alpha");
    }
}
