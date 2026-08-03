use std::path::{Path, PathBuf};

use russh_sftp::{client::SftpSession, protocol::OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    core::ssh::{executor::EventSink, transport::AuthenticatedSshSession},
    domain::events::{EventSequence, ExecutionEventPayload},
    error::{AppError, AppResult},
};

pub const TRANSFER_BLOCK_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadRequest {
    pub local_path: PathBuf,
    pub remote_path: String,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub remote_path: String,
    pub suggested_name: String,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferOutcome {
    pub bytes: u64,
    pub sha256: String,
    pub location: String,
}

pub fn validate_remote_path(path: &str) -> AppResult<()> {
    if path.is_empty() || path.contains('\0') || !path.starts_with('/') {
        return Err(AppError::Validation(
            "远程路径必须是无 NUL 的 POSIX 绝对路径".into(),
        ));
    }
    if path.split('/').any(|component| component == "..") {
        return Err(AppError::Validation("远程路径不能包含上级目录".into()));
    }
    Ok(())
}

pub fn remote_partial_path(remote_path: &str) -> AppResult<String> {
    validate_remote_path(remote_path)?;
    let (directory, name) = remote_path
        .rsplit_once('/')
        .ok_or_else(|| AppError::Validation("远程路径缺少文件名".into()))?;
    if name.is_empty() {
        return Err(AppError::Validation("远程路径缺少文件名".into()));
    }
    let directory = if directory.is_empty() { "/" } else { directory };
    let separator = if directory == "/" { "" } else { "/" };
    Ok(format!(
        "{directory}{separator}.qingzhou-{name}.{}.partial",
        Uuid::new_v4()
    ))
}

pub fn download_destination(data_root: &Path, suggested_name: &str) -> AppResult<PathBuf> {
    if suggested_name.is_empty()
        || suggested_name.contains('\0')
        || suggested_name == "."
        || suggested_name == ".."
        || suggested_name.contains('/')
        || suggested_name.contains('\\')
        || Path::new(suggested_name).is_absolute()
    {
        return Err(AppError::Validation("下载文件名无效".into()));
    }
    Ok(data_root.join("downloads").join(suggested_name))
}

pub fn local_partial_path(destination: &Path) -> PathBuf {
    let mut name = destination.file_name().unwrap_or_default().to_os_string();
    name.push(".partial");
    destination.with_file_name(name)
}

pub async fn sha256_local_file(path: &Path) -> AppResult<String> {
    let mut file = File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut block = vec![0_u8; TRANSFER_BLOCK_BYTES];
    loop {
        let read = file.read(&mut block).await?;
        if read == 0 {
            break;
        }
        hasher.update(&block[..read]);
    }
    Ok(hex_digest(hasher))
}

pub async fn upload<E: EventSink>(
    ssh: &AuthenticatedSshSession,
    request: &UploadRequest,
    events: &mut E,
    cancel: CancellationToken,
) -> AppResult<TransferOutcome> {
    validate_upload_request(request).await?;
    let total = tokio::fs::metadata(&request.local_path).await?.len();
    let partial_path = remote_partial_path(&request.remote_path)?;
    let sftp = open_sftp(ssh).await?;
    if sftp_try_exists(&sftp, &request.remote_path).await? && !request.overwrite {
        let _ = sftp.close().await;
        return Err(AppError::Validation("远程目标文件已存在".into()));
    }

    let result = upload_to_partial(&sftp, request, &partial_path, total, events, cancel).await;
    if result.is_err() {
        let _ = sftp.remove_file(partial_path.clone()).await;
    }
    let _ = sftp.close().await;
    result
}

async fn upload_to_partial<E: EventSink>(
    sftp: &SftpSession,
    request: &UploadRequest,
    partial_path: &str,
    total: u64,
    events: &mut E,
    cancel: CancellationToken,
) -> AppResult<TransferOutcome> {
    let mut local = File::open(&request.local_path).await?;
    let mut remote = sftp
        .open_with_flags(
            partial_path,
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
        )
        .await
        .map_err(sftp_error)?;
    let mut hasher = Sha256::new();
    let mut block = vec![0_u8; TRANSFER_BLOCK_BYTES];
    let mut transferred = 0_u64;
    let mut sequence = EventSequence::default();
    loop {
        let read = tokio::select! {
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
            result = local.read(&mut block) => result?,
        };
        if read == 0 {
            break;
        }
        tokio::select! {
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
            result = remote.write_all(&block[..read]) => result?,
        }
        hasher.update(&block[..read]);
        transferred = transferred.saturating_add(read as u64);
        emit_progress(events, &mut sequence, transferred, Some(total))?;
    }
    remote.flush().await?;
    remote.shutdown().await?;
    let local_hash = hex_digest(hasher);
    let remote_hash = hash_remote_with_sftp(sftp, partial_path, cancel.clone()).await?;
    if local_hash != remote_hash {
        return Err(AppError::Integrity(format!(
            "上传文件 SHA-256 不一致：本地 {local_hash}，远端 {remote_hash}"
        )));
    }
    if request.overwrite && sftp_try_exists(sftp, &request.remote_path).await? {
        sftp.remove_file(request.remote_path.clone())
            .await
            .map_err(sftp_error)?;
    }
    sftp.rename(partial_path, request.remote_path.clone())
        .await
        .map_err(sftp_error)?;
    Ok(TransferOutcome {
        bytes: transferred,
        sha256: local_hash,
        location: request.remote_path.clone(),
    })
}

pub async fn download<E: EventSink>(
    ssh: &AuthenticatedSshSession,
    data_root: &Path,
    request: &DownloadRequest,
    events: &mut E,
    cancel: CancellationToken,
) -> AppResult<TransferOutcome> {
    validate_remote_path(&request.remote_path)?;
    let destination = download_destination(data_root, &request.suggested_name)?;
    if destination.exists() && !request.overwrite {
        return Err(AppError::Validation("本地下载目标已存在".into()));
    }
    let partial = local_partial_path(&destination);
    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if partial.exists() {
        tokio::fs::remove_file(&partial).await?;
    }

    let sftp = open_sftp(ssh).await?;
    let result = download_to_partial(
        &sftp,
        data_root,
        request,
        &destination,
        &partial,
        events,
        cancel,
    )
    .await;
    if result.is_err() && partial.exists() {
        let _ = tokio::fs::remove_file(&partial).await;
    }
    let _ = sftp.close().await;
    result
}

async fn download_to_partial<E: EventSink>(
    sftp: &SftpSession,
    data_root: &Path,
    request: &DownloadRequest,
    destination: &Path,
    partial: &Path,
    events: &mut E,
    cancel: CancellationToken,
) -> AppResult<TransferOutcome> {
    let expected_hash = hash_remote_with_sftp(sftp, &request.remote_path, cancel.clone()).await?;
    let total = sftp
        .metadata(request.remote_path.clone())
        .await
        .map_err(sftp_error)?
        .size;
    let mut remote = sftp
        .open(request.remote_path.clone())
        .await
        .map_err(sftp_error)?;
    let mut local = File::create(partial).await?;
    let mut hasher = Sha256::new();
    let mut block = vec![0_u8; TRANSFER_BLOCK_BYTES];
    let mut transferred = 0_u64;
    let mut sequence = EventSequence::default();
    loop {
        let read = tokio::select! {
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
            result = remote.read(&mut block) => result?,
        };
        if read == 0 {
            break;
        }
        tokio::select! {
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
            result = local.write_all(&block[..read]) => result?,
        }
        hasher.update(&block[..read]);
        transferred = transferred.saturating_add(read as u64);
        emit_progress(events, &mut sequence, transferred, total)?;
    }
    local.flush().await?;
    local.sync_all().await?;
    let actual_hash = hex_digest(hasher);
    if expected_hash != actual_hash {
        return Err(AppError::Integrity(format!(
            "下载文件 SHA-256 不一致：远端 {expected_hash}，本地 {actual_hash}"
        )));
    }
    if request.overwrite && destination.exists() {
        tokio::fs::remove_file(destination).await?;
    }
    tokio::fs::rename(partial, destination).await?;
    let relative = destination
        .strip_prefix(data_root)
        .map_err(|_| AppError::Security("下载目标逃逸数据根目录".into()))?;
    Ok(TransferOutcome {
        bytes: transferred,
        sha256: actual_hash,
        location: relative.to_string_lossy().replace('\\', "/"),
    })
}

pub async fn hash_remote_file(
    ssh: &AuthenticatedSshSession,
    remote_path: &str,
    cancel: CancellationToken,
) -> AppResult<String> {
    validate_remote_path(remote_path)?;
    let sftp = open_sftp(ssh).await?;
    let result = hash_remote_with_sftp(&sftp, remote_path, cancel).await;
    let _ = sftp.close().await;
    result
}

async fn hash_remote_with_sftp(
    sftp: &SftpSession,
    remote_path: &str,
    cancel: CancellationToken,
) -> AppResult<String> {
    let mut remote = sftp.open(remote_path).await.map_err(sftp_error)?;
    let mut hasher = Sha256::new();
    let mut block = vec![0_u8; TRANSFER_BLOCK_BYTES];
    loop {
        let read = tokio::select! {
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
            result = remote.read(&mut block) => result?,
        };
        if read == 0 {
            break;
        }
        hasher.update(&block[..read]);
    }
    Ok(hex_digest(hasher))
}

async fn open_sftp(ssh: &AuthenticatedSshSession) -> AppResult<SftpSession> {
    let channel = ssh.open_session_channel().await?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(AppError::from)?;
    SftpSession::new(channel.into_stream())
        .await
        .map_err(sftp_error)
}

async fn validate_upload_request(request: &UploadRequest) -> AppResult<()> {
    validate_remote_path(&request.remote_path)?;
    if !request.local_path.is_absolute() || request.local_path.to_string_lossy().contains('\0') {
        return Err(AppError::Validation("上传源必须是绝对本地路径".into()));
    }
    let metadata = tokio::fs::metadata(&request.local_path).await?;
    if !metadata.is_file() {
        return Err(AppError::Validation("上传源必须是单个文件".into()));
    }
    Ok(())
}

async fn sftp_try_exists(sftp: &SftpSession, path: &str) -> AppResult<bool> {
    sftp.try_exists(path).await.map_err(sftp_error)
}

fn emit_progress<E: EventSink>(
    events: &mut E,
    sequence: &mut EventSequence,
    transferred: u64,
    total: Option<u64>,
) -> AppResult<()> {
    let percent = total
        .filter(|total| *total > 0)
        .map(|total| ((transferred as f64 / total as f64) * 100.0).clamp(0.0, 100.0));
    events.send(sequence.next(ExecutionEventPayload::Progress {
        transferred,
        total,
        percent,
    }))
}

fn hex_digest(hasher: Sha256) -> String {
    format!("{:x}", hasher.finalize())
}

fn sftp_error(error: impl std::fmt::Display) -> AppError {
    AppError::Transfer(error.to_string())
}
