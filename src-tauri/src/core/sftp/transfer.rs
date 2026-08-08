use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use russh_sftp::{
    client::{
        error::Error as SftpClientError, rawsession::Limits, Config as SftpConfig, RawSftpSession,
        SftpSession,
    },
    extensions,
    protocol::{FileAttributes, OpenFlags, StatusCode},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{
    pipeline::{
        effective_read_chunk, plan_window_from, Chunk, OrderedChunkBuffer, PipelineStats,
        MAX_PIPELINE_BYTES, MAX_PIPELINE_REQUESTS,
    },
    progress::{ProgressSnapshot, TransferPhase, TransferProgressTracker},
};

use crate::{
    core::{
        ssh::{
            executor::{EventSink, VecEventSink},
            transport::{execute_authenticated, AuthenticatedSshSession},
        },
        system_probe::SystemCapabilities,
        tasks::{prepare_task_restore_destination, shell_quote},
        workflows::resolve_restore_point_path,
    },
    domain::{
        events::{EventSequence, ExecutionEventPayload},
        execution::now_millis,
    },
    error::{AppError, AppResult},
};

pub const TRANSFER_BLOCK_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationPolicy {
    #[default]
    Balanced,
    Strict,
    TransportOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationLevel {
    RemoteHash,
    TransportAndSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationStrategy {
    RemoteHash,
    SftpReread,
    TransportAndSize,
}

pub fn select_verification(
    policy: VerificationPolicy,
    remote_hash_available: bool,
) -> VerificationStrategy {
    match (policy, remote_hash_available) {
        (VerificationPolicy::TransportOnly, _) => VerificationStrategy::TransportAndSize,
        (_, true) => VerificationStrategy::RemoteHash,
        (VerificationPolicy::Balanced, false) => VerificationStrategy::TransportAndSize,
        (VerificationPolicy::Strict, false) => VerificationStrategy::SftpReread,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadRequest {
    pub local_path: PathBuf,
    pub remote_path: String,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub verification: VerificationPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub remote_path: String,
    pub suggested_name: String,
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub verification: VerificationPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferOutcome {
    pub bytes: u64,
    pub sha256: String,
    pub location: String,
    pub verification_level: VerificationLevel,
    pub remote_hash_compared: bool,
    pub pipeline_max_in_flight: usize,
    pub pipeline_max_buffered_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteFileMetadata {
    pub size: Option<u64>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub permissions: Option<u32>,
    pub modified_at: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OperationFileBackup {
    pub transfer: Option<TransferOutcome>,
    pub metadata: Option<RemoteFileMetadata>,
}

pub fn validate_remote_path(path: &str) -> AppResult<()> {
    let windows_drive_absolute = {
        let bytes = path.as_bytes();
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
    };
    if path.is_empty()
        || path.contains('\0')
        || path.contains('\\')
        || !(path.starts_with('/') || windows_drive_absolute)
    {
        return Err(AppError::Validation(
            "远程路径必须是无 NUL、使用正斜杠的 SFTP 绝对路径".into(),
        ));
    }
    if path.split('/').any(|component| component == "..") {
        return Err(AppError::Validation("远程路径不能包含上级目录".into()));
    }
    Ok(())
}

pub fn remote_hash_command(
    capabilities: &SystemCapabilities,
    remote_path: &str,
) -> AppResult<Option<String>> {
    validate_remote_path(remote_path)?;
    if capabilities.platform_family == crate::core::system_probe::RemoteOsFamily::Windows
        || capabilities.path_style == crate::core::system_probe::RemotePathStyle::WindowsSftp
        || !remote_path.starts_with('/')
    {
        return Ok(None);
    }
    let quoted = shell_quote(remote_path);
    Ok(if capabilities.has_command("sha256sum") {
        Some(format!("sha256sum -- {quoted}"))
    } else if capabilities.has_command("sha256") {
        Some(format!("sha256 -q {quoted}"))
    } else if capabilities.has_command("shasum") {
        Some(format!("shasum -a 256 -- {quoted}"))
    } else {
        None
    })
}

pub fn parse_sha256_output(output: &str) -> AppResult<String> {
    let line = output.strip_suffix('\n').unwrap_or(output);
    let line = line.strip_suffix('\r').unwrap_or(line);
    if line.contains('\r') || line.contains('\n') {
        return Err(AppError::Integrity(
            "远端 SHA-256 输出包含多行或歧义内容".into(),
        ));
    }
    let (digest, trailing) = line
        .find(char::is_whitespace)
        .map(|index| (&line[..index], Some(line[index..].trim_start())))
        .unwrap_or((line, None));
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::Integrity("远端 SHA-256 输出格式无效".into()));
    }
    if trailing.is_some_and(|path| path.is_empty() || path.contains('\0')) {
        return Err(AppError::Integrity("远端 SHA-256 输出格式无效".into()));
    }
    Ok(digest.to_ascii_lowercase())
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
    capabilities: &SystemCapabilities,
    request: &UploadRequest,
    events: &mut E,
    cancel: CancellationToken,
) -> AppResult<TransferOutcome> {
    validate_upload_request(request).await?;
    let verification = select_verification(
        request.verification,
        remote_hash_command(capabilities, &request.remote_path)?.is_some(),
    );
    let total = tokio::fs::metadata(&request.local_path).await?.len();
    let partial_path = remote_partial_path(&request.remote_path)?;
    let sftp = open_sftp(ssh).await?;
    if sftp_try_exists(&sftp, &request.remote_path).await? && !request.overwrite {
        let _ = sftp.close().await;
        return Err(AppError::Validation("远程目标文件已存在".into()));
    }

    let result = upload_to_partial(
        ssh,
        capabilities,
        &sftp,
        request,
        &partial_path,
        total,
        verification,
        events,
        cancel,
    )
    .await;
    if result.is_err() {
        let _ = sftp.remove_file(partial_path.clone()).await;
    }
    let _ = sftp.close().await;
    result
}

async fn upload_to_partial<E: EventSink>(
    ssh: &AuthenticatedSshSession,
    capabilities: &SystemCapabilities,
    sftp: &SftpSession,
    request: &UploadRequest,
    partial_path: &str,
    total: u64,
    verification: VerificationStrategy,
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
    let mut progress = TransferProgressTracker::new(Some(total), now_millis());
    if let Some(snapshot) = progress.sample(0, now_millis()) {
        emit_progress(events, &mut sequence, snapshot)?;
    }
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
        if let Some(snapshot) = progress.sample(transferred, now_millis()) {
            emit_progress(events, &mut sequence, snapshot)?;
        }
    }
    remote.flush().await?;
    remote.shutdown().await?;
    emit_progress(
        events,
        &mut sequence,
        progress.change_phase(TransferPhase::Verifying, now_millis()),
    )?;
    let local_hash = hex_digest(hasher);
    let remote_size = sftp.metadata(partial_path).await.map_err(sftp_error)?.size;
    if verification == VerificationStrategy::TransportAndSize && remote_size.is_none() {
        return Err(AppError::Integrity("远端未返回上传文件大小".into()));
    }
    if remote_size.is_some_and(|remote_size| remote_size != transferred) {
        return Err(AppError::Integrity(format!(
            "上传文件大小不一致：本地 {transferred} 字节，远端 {} 字节",
            remote_size.unwrap_or_default()
        )));
    }
    let remote_hash = match verification {
        VerificationStrategy::RemoteHash => {
            Some(hash_remote_with_command(ssh, capabilities, partial_path, cancel.clone()).await?)
        }
        VerificationStrategy::SftpReread => {
            Some(hash_remote_with_sftp(sftp, partial_path, cancel.clone()).await?)
        }
        VerificationStrategy::TransportAndSize => None,
    };
    if let Some(remote_hash) = &remote_hash {
        if local_hash != *remote_hash {
            return Err(AppError::Integrity(format!(
                "上传文件 SHA-256 不一致：本地 {local_hash}，远端 {remote_hash}"
            )));
        }
    }
    emit_progress(
        events,
        &mut sequence,
        progress.change_phase(TransferPhase::Finalizing, now_millis()),
    )?;
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
        verification_level: verification_level(verification),
        remote_hash_compared: remote_hash.is_some(),
        pipeline_max_in_flight: 0,
        pipeline_max_buffered_bytes: 0,
    })
}

pub async fn download<E: EventSink>(
    ssh: &AuthenticatedSshSession,
    capabilities: &SystemCapabilities,
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
    let verification = select_verification(
        request.verification,
        remote_hash_command(capabilities, &request.remote_path)?.is_some(),
    );
    let total = sftp
        .metadata(&request.remote_path)
        .await
        .map_err(sftp_error)?
        .size;
    let mut progress = TransferProgressTracker::new(total, now_millis());
    let mut sequence = EventSequence::default();
    if verification != VerificationStrategy::TransportAndSize {
        emit_progress(
            events,
            &mut sequence,
            progress.change_phase(TransferPhase::Verifying, now_millis()),
        )?;
    }
    let expected_hash = match verification {
        VerificationStrategy::RemoteHash => Some(
            hash_remote_with_command(ssh, capabilities, &request.remote_path, cancel.clone())
                .await?,
        ),
        VerificationStrategy::SftpReread => {
            Some(hash_remote_with_sftp(&sftp, &request.remote_path, cancel.clone()).await?)
        }
        VerificationStrategy::TransportAndSize => None,
    };
    emit_progress(
        events,
        &mut sequence,
        progress.change_phase(TransferPhase::Transferring, now_millis()),
    )?;
    let target = LocalDownloadTarget {
        data_root,
        destination: &destination,
        partial: &partial,
        overwrite: request.overwrite,
    };
    let result = download_to_partial(
        ssh,
        &sftp,
        &request.remote_path,
        target,
        expected_hash,
        verification,
        total,
        &mut progress,
        &mut sequence,
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

struct LocalDownloadTarget<'a> {
    data_root: &'a Path,
    destination: &'a Path,
    partial: &'a Path,
    overwrite: bool,
}

async fn download_to_partial<E: EventSink>(
    ssh: &AuthenticatedSshSession,
    sftp: &SftpSession,
    remote_path: &str,
    target: LocalDownloadTarget<'_>,
    expected_hash: Option<String>,
    verification: VerificationStrategy,
    total: Option<u64>,
    progress: &mut TransferProgressTracker,
    sequence: &mut EventSequence,
    events: &mut E,
    cancel: CancellationToken,
) -> AppResult<TransferOutcome> {
    let mut local = File::create(target.partial).await?;
    let mut hasher = Sha256::new();
    let (transferred, pipeline_stats) = match total {
        Some(total) => {
            download_known_size_pipelined(
                ssh,
                remote_path,
                total,
                &mut local,
                &mut hasher,
                progress,
                sequence,
                events,
                cancel.clone(),
            )
            .await?
        }
        None => (
            download_unknown_size_sequential(
                sftp,
                remote_path,
                &mut local,
                &mut hasher,
                progress,
                sequence,
                events,
                cancel.clone(),
            )
            .await?,
            PipelineStats::default(),
        ),
    };
    local.flush().await?;
    local.sync_all().await?;
    emit_progress(
        events,
        sequence,
        progress.change_phase(TransferPhase::Verifying, now_millis()),
    )?;
    let actual_hash = hex_digest(hasher);
    if verification == VerificationStrategy::TransportAndSize && total.is_none() {
        return Err(AppError::Integrity("远端未返回下载文件大小".into()));
    }
    if total.is_some_and(|total| total != transferred) {
        return Err(AppError::Integrity(format!(
            "下载文件大小不一致：远端 {} 字节，本地 {transferred} 字节",
            total.unwrap_or_default()
        )));
    }
    if let Some(expected_hash) = &expected_hash {
        if *expected_hash != actual_hash {
            return Err(AppError::Integrity(format!(
                "下载文件 SHA-256 不一致：远端 {expected_hash}，本地 {actual_hash}"
            )));
        }
    }
    emit_progress(
        events,
        sequence,
        progress.change_phase(TransferPhase::Finalizing, now_millis()),
    )?;
    if target.overwrite && target.destination.exists() {
        tokio::fs::remove_file(target.destination).await?;
    }
    tokio::fs::rename(target.partial, target.destination).await?;
    let relative = target
        .destination
        .strip_prefix(target.data_root)
        .map_err(|_| AppError::Security("下载目标逃逸数据根目录".into()))?;
    Ok(TransferOutcome {
        bytes: transferred,
        sha256: actual_hash,
        location: relative.to_string_lossy().replace('\\', "/"),
        verification_level: verification_level(verification),
        remote_hash_compared: expected_hash.is_some(),
        pipeline_max_in_flight: pipeline_stats.max_in_flight,
        pipeline_max_buffered_bytes: pipeline_stats.max_buffered_bytes,
    })
}

#[allow(clippy::too_many_arguments)]
async fn download_unknown_size_sequential<E: EventSink>(
    sftp: &SftpSession,
    remote_path: &str,
    local: &mut File,
    hasher: &mut Sha256,
    progress: &mut TransferProgressTracker,
    sequence: &mut EventSequence,
    events: &mut E,
    cancel: CancellationToken,
) -> AppResult<u64> {
    let mut remote = sftp.open(remote_path).await.map_err(sftp_error)?;
    let mut block = vec![0_u8; TRANSFER_BLOCK_BYTES];
    let mut transferred = 0_u64;
    loop {
        let read = tokio::select! {
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
            result = remote.read(&mut block) => result?,
        };
        if read == 0 {
            break;
        }
        write_download_chunk(
            local,
            hasher,
            progress,
            sequence,
            events,
            &cancel,
            &mut transferred,
            &block[..read],
        )
        .await?;
    }
    Ok(transferred)
}

#[allow(clippy::too_many_arguments)]
async fn download_known_size_pipelined<E: EventSink>(
    ssh: &AuthenticatedSshSession,
    remote_path: &str,
    total: u64,
    local: &mut File,
    hasher: &mut Sha256,
    progress: &mut TransferProgressTracker,
    sequence: &mut EventSequence,
    events: &mut E,
    cancel: CancellationToken,
) -> AppResult<(u64, PipelineStats)> {
    let (session, handle, chunk_bytes) = open_raw_download(ssh, remote_path).await?;
    let result = run_download_pipeline(
        session.clone(),
        &handle,
        chunk_bytes,
        total,
        local,
        hasher,
        progress,
        sequence,
        events,
        cancel,
    )
    .await;
    let _ = session.close(handle).await;
    let _ = session.close_session();
    result
}

#[allow(clippy::too_many_arguments)]
async fn run_download_pipeline<E: EventSink>(
    session: Arc<RawSftpSession>,
    handle: &str,
    chunk_bytes: u64,
    total: u64,
    local: &mut File,
    hasher: &mut Sha256,
    progress: &mut TransferProgressTracker,
    sequence: &mut EventSequence,
    events: &mut E,
    cancel: CancellationToken,
) -> AppResult<(u64, PipelineStats)> {
    let mut transferred = 0_u64;
    let mut stats = PipelineStats::default();
    while transferred < total {
        let chunks = plan_window_from(transferred, total, chunk_bytes, MAX_PIPELINE_REQUESTS);
        let window_end = chunks
            .last()
            .map(|chunk| chunk.offset.saturating_add(chunk.len as u64))
            .ok_or_else(|| AppError::Integrity("SFTP 下载流水线无法规划剩余文件".into()))?;
        let mut reads = JoinSet::new();
        for chunk in chunks {
            spawn_pipeline_read(&mut reads, session.clone(), handle.to_owned(), chunk);
        }
        let mut ordered = OrderedChunkBuffer::new(transferred);
        stats.observe(reads.len(), ordered.buffered_bytes());
        while !reads.is_empty() {
            let joined = tokio::select! {
                _ = cancel.cancelled() => {
                    reads.abort_all();
                    return Err(AppError::Cancelled);
                }
                joined = reads.join_next() => joined,
            }
            .ok_or_else(|| AppError::Transfer("SFTP 下载流水线提前结束".into()))?
            .map_err(|error| AppError::Transfer(format!("SFTP 下载读取任务失败：{error}")))?;
            let (chunk, response) = joined;
            let data = match response {
                Ok(data) => data.data,
                Err(SftpClientError::Status(status)) if status.status_code == StatusCode::Eof => {
                    reads.abort_all();
                    return Err(AppError::Integrity(
                        "远端文件在元数据声明的大小之前结束".into(),
                    ));
                }
                Err(error) => {
                    reads.abort_all();
                    return Err(sftp_error(error));
                }
            };
            if data.is_empty() || data.len() > chunk.len as usize {
                reads.abort_all();
                return Err(AppError::Integrity("远端返回了无效的 SFTP 数据块".into()));
            }
            if data.len() < chunk.len as usize {
                let consumed = data.len() as u64;
                spawn_pipeline_read(
                    &mut reads,
                    session.clone(),
                    handle.to_owned(),
                    Chunk {
                        offset: chunk.offset.saturating_add(consumed),
                        len: chunk.len - data.len() as u32,
                    },
                );
            }
            if !ordered.insert(chunk.offset, data) {
                reads.abort_all();
                return Err(AppError::Integrity(
                    "远端返回了重复或重叠的 SFTP 数据块".into(),
                ));
            }
            stats.observe(reads.len(), ordered.buffered_bytes());
            if ordered.buffered_bytes() > MAX_PIPELINE_BYTES {
                reads.abort_all();
                return Err(AppError::Integrity(
                    "SFTP 下载流水线缓冲区超过安全上限".into(),
                ));
            }
            for (_, data) in ordered.drain_ready() {
                write_download_chunk(
                    local,
                    hasher,
                    progress,
                    sequence,
                    events,
                    &cancel,
                    &mut transferred,
                    &data,
                )
                .await?;
            }
        }
        if transferred != window_end || ordered.buffered_bytes() != 0 {
            return Err(AppError::Integrity(
                "SFTP 下载流水线未形成连续文件内容".into(),
            ));
        }
    }
    Ok((transferred, stats))
}

fn spawn_pipeline_read(
    reads: &mut JoinSet<(Chunk, Result<russh_sftp::protocol::Data, SftpClientError>)>,
    session: Arc<RawSftpSession>,
    handle: String,
    chunk: Chunk,
) {
    reads.spawn(async move {
        let response = session.read(handle, chunk.offset, chunk.len).await;
        (chunk, response)
    });
}

#[allow(clippy::too_many_arguments)]
async fn write_download_chunk<E: EventSink>(
    local: &mut File,
    hasher: &mut Sha256,
    progress: &mut TransferProgressTracker,
    sequence: &mut EventSequence,
    events: &mut E,
    cancel: &CancellationToken,
    transferred: &mut u64,
    data: &[u8],
) -> AppResult<()> {
    tokio::select! {
        _ = cancel.cancelled() => return Err(AppError::Cancelled),
        result = local.write_all(data) => result?,
    }
    hasher.update(data);
    *transferred = transferred.saturating_add(data.len() as u64);
    if let Some(snapshot) = progress.sample(*transferred, now_millis()) {
        emit_progress(events, sequence, snapshot)?;
    }
    Ok(())
}

pub(crate) async fn backup_remote_file(
    ssh: &AuthenticatedSshSession,
    data_root: &Path,
    remote_path: &str,
    relative_path: &str,
    cancel: CancellationToken,
) -> AppResult<Option<TransferOutcome>> {
    validate_remote_path(remote_path)?;
    let destination = resolve_restore_point_path(data_root, relative_path)?;
    backup_remote_file_to(
        ssh,
        data_root,
        remote_path,
        relative_path,
        &destination,
        cancel,
    )
    .await
    .map(|backup| backup.transfer)
}

pub(crate) async fn backup_operation_remote_file(
    ssh: &AuthenticatedSshSession,
    data_root: &Path,
    remote_path: &str,
    relative_path: &Path,
    cancel: CancellationToken,
) -> AppResult<OperationFileBackup> {
    validate_remote_path(remote_path)?;
    let destination = prepare_task_restore_destination(data_root, relative_path).await?;
    let location = relative_path.to_string_lossy().replace('\\', "/");
    backup_remote_file_to(ssh, data_root, remote_path, &location, &destination, cancel).await
}

async fn backup_remote_file_to(
    ssh: &AuthenticatedSshSession,
    data_root: &Path,
    remote_path: &str,
    relative_location: &str,
    destination: &Path,
    cancel: CancellationToken,
) -> AppResult<OperationFileBackup> {
    let partial = local_partial_path(destination);
    let sftp = open_sftp(ssh).await?;
    let metadata = match sftp.symlink_metadata(remote_path).await {
        Ok(metadata) => metadata,
        Err(metadata_error) => match sftp_try_exists(&sftp, remote_path).await {
            Ok(false) => {
                let _ = sftp.close().await;
                return Ok(OperationFileBackup {
                    transfer: None,
                    metadata: None,
                });
            }
            Ok(true) => {
                let _ = sftp.close().await;
                return Err(sftp_error(metadata_error));
            }
            Err(error) => {
                let _ = sftp.close().await;
                return Err(error);
            }
        },
    };
    if metadata.is_symlink() {
        let _ = sftp.close().await;
        return Err(AppError::Security(
            "远程恢复目标是符号链接，已拒绝备份".into(),
        ));
    }
    if !metadata.is_regular() {
        let _ = sftp.close().await;
        return Err(AppError::Validation("远程恢复目标必须是普通文件".into()));
    }
    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if let Ok(existing) = tokio::fs::symlink_metadata(destination).await {
        let _ = sftp.close().await;
        return Err(if existing.file_type().is_symlink() {
            AppError::Security("本地恢复资产目标是符号链接".into())
        } else {
            AppError::Security("本地恢复资产目标已经存在".into())
        });
    }
    if partial.exists() {
        let partial_metadata = tokio::fs::symlink_metadata(&partial).await?;
        if partial_metadata.file_type().is_symlink() || !partial_metadata.is_file() {
            let _ = sftp.close().await;
            return Err(AppError::Security("本地临时恢复资产不是普通文件".into()));
        }
        tokio::fs::remove_file(&partial).await?;
    }
    let mut events = VecEventSink::default();
    let target = LocalDownloadTarget {
        data_root,
        destination,
        partial: &partial,
        overwrite: true,
    };
    let mut progress = TransferProgressTracker::new(metadata.size, now_millis());
    let mut sequence = EventSequence::default();
    emit_progress(
        &mut events,
        &mut sequence,
        progress.change_phase(TransferPhase::Verifying, now_millis()),
    )?;
    let expected_hash = hash_remote_with_sftp(&sftp, remote_path, cancel.clone()).await?;
    emit_progress(
        &mut events,
        &mut sequence,
        progress.change_phase(TransferPhase::Transferring, now_millis()),
    )?;
    let result = download_to_partial(
        ssh,
        &sftp,
        remote_path,
        target,
        Some(expected_hash),
        VerificationStrategy::SftpReread,
        metadata.size,
        &mut progress,
        &mut sequence,
        &mut events,
        cancel,
    )
    .await;
    if result.is_err() && partial.exists() {
        let _ = tokio::fs::remove_file(&partial).await;
    }
    let _ = sftp.close().await;
    result.map(|transfer| OperationFileBackup {
        transfer: Some(TransferOutcome {
            location: relative_location.into(),
            ..transfer
        }),
        metadata: Some(RemoteFileMetadata {
            size: metadata.size,
            uid: metadata.uid,
            gid: metadata.gid,
            permissions: metadata.permissions,
            modified_at: metadata.mtime,
        }),
    })
}

pub(crate) async fn delete_remote_file(
    ssh: &AuthenticatedSshSession,
    remote_path: &str,
) -> AppResult<bool> {
    validate_remote_path(remote_path)?;
    let sftp = open_sftp(ssh).await?;
    let result = async {
        if !sftp_try_exists(&sftp, remote_path).await? {
            return Ok(false);
        }
        sftp.remove_file(remote_path).await.map_err(sftp_error)?;
        Ok(true)
    }
    .await;
    let _ = sftp.close().await;
    result
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

async fn hash_remote_with_command(
    ssh: &AuthenticatedSshSession,
    capabilities: &SystemCapabilities,
    remote_path: &str,
    cancel: CancellationToken,
) -> AppResult<String> {
    let command = remote_hash_command(capabilities, remote_path)?
        .ok_or_else(|| AppError::Compatibility("远端缺少固定的 SHA-256 校验命令".into()))?;
    let output = tokio::select! {
        _ = cancel.cancelled() => return Err(AppError::Cancelled),
        output = execute_authenticated(ssh, &command) => output?,
    };
    if output.exit_status != 0 {
        return Err(AppError::ssh_command(output.exit_status, output.stderr));
    }
    parse_sha256_output(&output.stdout)
}

fn verification_level(strategy: VerificationStrategy) -> VerificationLevel {
    match strategy {
        VerificationStrategy::RemoteHash | VerificationStrategy::SftpReread => {
            VerificationLevel::RemoteHash
        }
        VerificationStrategy::TransportAndSize => VerificationLevel::TransportAndSize,
    }
}

async fn open_sftp(ssh: &AuthenticatedSshSession) -> AppResult<SftpSession> {
    let channel = ssh.open_session_channel().await?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(AppError::from)?;
    SftpSession::new_with_config(channel.into_stream(), transfer_sftp_config(ssh))
        .await
        .map_err(sftp_error)
}

async fn open_raw_download(
    ssh: &AuthenticatedSshSession,
    remote_path: &str,
) -> AppResult<(Arc<RawSftpSession>, String, u64)> {
    let channel = ssh.open_session_channel().await?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(AppError::from)?;
    let mut session =
        RawSftpSession::new_with_config(channel.into_stream(), transfer_sftp_config(ssh));
    let version = session.init().await.map_err(sftp_error)?;
    let mut limits = Limits::default();
    if version
        .extensions
        .get(extensions::LIMITS)
        .is_some_and(|version| version == "1")
    {
        limits = session.limits().await.map_err(sftp_error)?.into();
        session.set_limits(limits);
    }
    let chunk_bytes = effective_read_chunk(limits.packet_len, limits.read_len);
    let handle = session
        .open(remote_path, OpenFlags::READ, FileAttributes::default())
        .await
        .map_err(sftp_error)?
        .handle;
    Ok((Arc::new(session), handle, chunk_bytes))
}

fn transfer_sftp_config(ssh: &AuthenticatedSshSession) -> SftpConfig {
    SftpConfig {
        max_packet_len: 2 * 1024 * 1024,
        max_concurrent_writes: MAX_PIPELINE_REQUESTS,
        request_timeout_secs: ssh.timeout().as_secs().max(1),
    }
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
    snapshot: ProgressSnapshot,
) -> AppResult<()> {
    events.send(sequence.next(ExecutionEventPayload::Progress {
        phase: snapshot.phase,
        transferred: snapshot.transferred,
        total: snapshot.total,
        percent: snapshot.percent,
        bytes_per_second: snapshot.bytes_per_second,
        average_bytes_per_second: snapshot.average_bytes_per_second,
        eta_seconds: snapshot.eta_seconds,
    }))
}

fn hex_digest(hasher: Sha256) -> String {
    format!("{:x}", hasher.finalize())
}

fn sftp_error(error: impl std::fmt::Display) -> AppError {
    AppError::Transfer(error.to_string())
}
