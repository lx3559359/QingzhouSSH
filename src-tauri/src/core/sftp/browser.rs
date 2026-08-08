use std::path::{Path, PathBuf};

use russh_sftp::client::SftpSession;
use serde::Serialize;

use crate::{
    core::ssh::transport::AuthenticatedSshSession,
    error::{AppError, AppResult},
};

use super::validate_remote_path;

const MAX_DIRECTORY_ENTRIES: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
    Some(if parent.is_empty() { "/" } else { parent }.to_string())
}

pub async fn list_remote_directory(
    ssh: &AuthenticatedSshSession,
    path: &str,
) -> AppResult<DirectoryListing> {
    validate_remote_directory_path(path)?;
    let sftp = open_sftp(ssh).await?;
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
