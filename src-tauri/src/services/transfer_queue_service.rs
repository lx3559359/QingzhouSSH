use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex as StdMutex,
};
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{mpsc, Notify};
use uuid::Uuid;

use crate::{
    core::{
        sftp::{self, DownloadRequest, UploadRequest, VerificationPolicy},
        ssh::executor::EventSink,
    },
    domain::{
        events::{ExecutionEvent, ExecutionEventPayload},
        execution::{ExecutionDetails, ExecutionStatus},
        transfer_job::{
            can_automatically_retry, NewTransferJob, TransferDirection, TransferJob,
            TransferJobStatus,
        },
    },
    error::{AppError, AppResult},
    repositories::transfer_job_repository::{
        CancelTransferAction, TransferJobFinish, TransferJobRepository,
    },
    services::{execution_service::ExecutionRegistry, transfer_service::TransferService},
};

const QUEUE_WORKERS: usize = 3;

#[derive(Clone)]
pub struct TransferQueueService {
    repository: TransferJobRepository,
    transfers: TransferService,
    registry: ExecutionRegistry,
    notify: Arc<Notify>,
    started: Arc<AtomicBool>,
}

impl TransferQueueService {
    pub fn new(
        repository: TransferJobRepository,
        transfers: TransferService,
        registry: ExecutionRegistry,
    ) -> Self {
        Self {
            repository,
            transfers,
            registry,
            notify: Arc::new(Notify::new()),
            started: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start(&self) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        for _ in 0..QUEUE_WORKERS {
            let service = self.clone();
            tokio::spawn(async move { service.worker_loop().await });
        }
    }

    pub async fn is_idle(&self) -> bool {
        !self.repository.has_pending().await.unwrap_or(true)
    }

    pub async fn enqueue_upload(
        &self,
        server_id: &str,
        request: UploadRequest,
    ) -> AppResult<TransferJob> {
        sftp::validate_remote_path(&request.remote_path)?;
        let job = self
            .repository
            .create(NewTransferJob {
                server_id: server_id.into(),
                direction: TransferDirection::Upload,
                source_path: request.local_path.to_string_lossy().into_owned(),
                target_path: request.remote_path,
                overwrite: request.overwrite,
                verification: verification_name(request.verification).into(),
            })
            .await?;
        self.wake_workers();
        Ok(job)
    }

    pub async fn enqueue_download(
        &self,
        server_id: &str,
        request: DownloadRequest,
    ) -> AppResult<TransferJob> {
        sftp::validate_remote_path(&request.remote_path)?;
        if request.suggested_name.trim().is_empty()
            || request
                .suggested_name
                .chars()
                .any(|value| matches!(value, '/' | '\\' | '\0'))
        {
            return Err(AppError::Validation("下载文件名无效".into()));
        }
        let job = self
            .repository
            .create(NewTransferJob {
                server_id: server_id.into(),
                direction: TransferDirection::Download,
                source_path: request.remote_path,
                target_path: request.suggested_name,
                overwrite: request.overwrite,
                verification: verification_name(request.verification).into(),
            })
            .await?;
        self.wake_workers();
        Ok(job)
    }

    pub async fn list(&self, server_id: Option<&str>) -> AppResult<Vec<TransferJob>> {
        self.repository.list(server_id).await
    }

    pub async fn cancel(&self, id: Uuid) -> AppResult<TransferJob> {
        match self.repository.request_cancel(id).await? {
            CancelTransferAction::CancelledQueued | CancelTransferAction::AwaitExecutionId => {}
            CancelTransferAction::SignalExecution(execution_id) => {
                if let Err(error) = self.registry.cancel(execution_id).await {
                    let current = self.repository.require(id).await?;
                    if !current.status.is_terminal() {
                        return Err(error);
                    }
                }
            }
        }
        self.repository.require(id).await
    }

    pub async fn retry(&self, id: Uuid) -> AppResult<TransferJob> {
        let job = self.repository.retry(id).await?;
        self.wake_workers();
        Ok(job)
    }

    fn wake_workers(&self) {
        for _ in 0..QUEUE_WORKERS {
            self.notify.notify_one();
        }
    }

    async fn worker_loop(self) {
        loop {
            match self.repository.next_runnable().await {
                Ok(Some(job)) => match self.repository.claim(job.id).await {
                    Ok(true) => self.execute(job.id).await,
                    Ok(false) => tokio::task::yield_now().await,
                    Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
                },
                Ok(None) => self.notify.notified().await,
                Err(_) => tokio::time::sleep(Duration::from_secs(1)).await,
            }
        }
    }

    async fn execute(&self, id: Uuid) {
        let result = self.execute_inner(id).await;
        if let Err(error) = result {
            let _ = self
                .repository
                .finish(
                    id,
                    TransferJobFinish {
                        status: TransferJobStatus::Failed,
                        retryable: error.retryable(),
                        error_category: Some(error.code().into()),
                        error_message: Some(error.to_string()),
                        sha256: None,
                        location: None,
                    },
                )
                .await;
        }

        if let Ok(job) = self.repository.require(id).await {
            if job.status == TransferJobStatus::Failed
                && job.retryable
                && can_automatically_retry(
                    job.error_category.as_deref().unwrap_or_default(),
                    job.error_message.as_deref().unwrap_or_default(),
                    job.attempt_count,
                    job.max_attempts,
                )
            {
                let delay = Duration::from_millis(250 * u64::from(job.attempt_count.max(1)));
                tokio::time::sleep(delay).await;
                let _ = self.repository.requeue_automatically(id).await;
            }
        }
        self.wake_workers();
    }

    async fn execute_inner(&self, id: Uuid) -> AppResult<()> {
        let job = self.repository.require(id).await?;
        let verification = parse_verification(&job.verification)?;
        let (sender, receiver) = mpsc::unbounded_channel();
        let result_capture = Arc::new(StdMutex::new(None));
        let mut sink = QueueEventSink {
            sender,
            result: result_capture.clone(),
        };
        let updater = tokio::spawn(process_events(
            id,
            self.repository.clone(),
            self.registry.clone(),
            receiver,
        ));

        let details = match job.direction {
            TransferDirection::Upload => {
                self.transfers
                    .upload(
                        &job.server_id,
                        UploadRequest {
                            local_path: job.source_path.into(),
                            remote_path: job.target_path,
                            overwrite: job.overwrite,
                            verification,
                        },
                        &mut sink,
                    )
                    .await
            }
            TransferDirection::Download => {
                self.transfers
                    .download(
                        &job.server_id,
                        DownloadRequest {
                            remote_path: job.source_path,
                            suggested_name: job.target_path,
                            overwrite: job.overwrite,
                            verification,
                        },
                        &mut sink,
                    )
                    .await
            }
        };
        drop(sink);
        updater
            .await
            .map_err(|error| AppError::Transfer(format!("传输状态更新任务异常：{error}")))??;
        let details = details?;
        self.ensure_finished(id, &details, result_capture).await
    }

    async fn ensure_finished(
        &self,
        id: Uuid,
        details: &ExecutionDetails,
        result_capture: Arc<StdMutex<Option<Value>>>,
    ) -> AppResult<()> {
        if self.repository.require(id).await?.status.is_terminal() {
            return Ok(());
        }
        let result = result_capture
            .lock()
            .map_err(|_| AppError::Transfer("传输结果锁已损坏".into()))?
            .clone();
        let (sha256, location) = result_fields(result.as_ref());
        self.repository
            .finish(
                id,
                TransferJobFinish {
                    status: execution_status(details.record.status),
                    retryable: details.record.retryable,
                    error_category: details.record.error_category.clone(),
                    error_message: details.record.error_message.clone(),
                    sha256,
                    location,
                },
            )
            .await
    }
}

struct QueueEventSink {
    sender: mpsc::UnboundedSender<ExecutionEvent>,
    result: Arc<StdMutex<Option<Value>>>,
}

impl EventSink for QueueEventSink {
    fn send(&mut self, event: ExecutionEvent) -> AppResult<()> {
        if let ExecutionEventPayload::Finished { result, .. } = &event.payload {
            if let Ok(mut captured) = self.result.lock() {
                *captured = result.clone();
            }
        }
        self.sender
            .send(event)
            .map_err(|_| AppError::Transfer("传输状态通道已经关闭".into()))
    }
}

async fn process_events(
    job_id: Uuid,
    repository: TransferJobRepository,
    registry: ExecutionRegistry,
    mut receiver: mpsc::UnboundedReceiver<ExecutionEvent>,
) -> AppResult<()> {
    while let Some(event) = receiver.recv().await {
        match event.payload {
            ExecutionEventPayload::Started { execution_id, .. } => {
                repository.set_execution(job_id, execution_id).await?;
                if repository.should_cancel(job_id).await? {
                    let _ = registry.cancel(execution_id).await;
                }
            }
            ExecutionEventPayload::Progress {
                phase,
                transferred,
                total,
                percent,
                bytes_per_second,
                average_bytes_per_second,
                eta_seconds,
            } => {
                repository
                    .update_progress(
                        job_id,
                        TransferJobStatus::from(phase),
                        transferred,
                        total,
                        percent,
                        bytes_per_second,
                        average_bytes_per_second,
                        eta_seconds,
                    )
                    .await?;
            }
            ExecutionEventPayload::Finished { status, result, .. } => {
                let (sha256, location) = result_fields(result.as_ref());
                repository
                    .finish(
                        job_id,
                        TransferJobFinish {
                            status: execution_status(status),
                            retryable: false,
                            error_category: None,
                            error_message: None,
                            sha256,
                            location,
                        },
                    )
                    .await?;
            }
            ExecutionEventPayload::Failed {
                category,
                message,
                retryable,
            } => {
                let status = match category.as_str() {
                    "cancelled" => TransferJobStatus::Cancelled,
                    "remote_state_uncertain" => TransferJobStatus::Uncertain,
                    _ => TransferJobStatus::Failed,
                };
                repository
                    .finish(
                        job_id,
                        TransferJobFinish {
                            status,
                            retryable,
                            error_category: Some(category),
                            error_message: Some(message),
                            sha256: None,
                            location: None,
                        },
                    )
                    .await?;
            }
            ExecutionEventPayload::Stdout { .. }
            | ExecutionEventPayload::Stderr { .. }
            | ExecutionEventPayload::FileProduced { .. } => {}
        }
    }
    Ok(())
}

fn execution_status(status: ExecutionStatus) -> TransferJobStatus {
    match status {
        ExecutionStatus::Succeeded => TransferJobStatus::Succeeded,
        ExecutionStatus::Cancelled => TransferJobStatus::Cancelled,
        ExecutionStatus::Uncertain => TransferJobStatus::Uncertain,
        ExecutionStatus::Queued | ExecutionStatus::Running | ExecutionStatus::Failed => {
            TransferJobStatus::Failed
        }
    }
}

fn result_fields(result: Option<&Value>) -> (Option<String>, Option<String>) {
    let sha256 = result
        .and_then(|value| value.get("sha256"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let location = result
        .and_then(|value| value.get("location"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    (sha256, location)
}

fn verification_name(policy: VerificationPolicy) -> &'static str {
    match policy {
        VerificationPolicy::Balanced => "balanced",
        VerificationPolicy::Strict => "strict",
        VerificationPolicy::TransportOnly => "transport_only",
    }
}

fn parse_verification(value: &str) -> AppResult<VerificationPolicy> {
    match value {
        "balanced" => Ok(VerificationPolicy::Balanced),
        "strict" => Ok(VerificationPolicy::Strict),
        "transport_only" => Ok(VerificationPolicy::TransportOnly),
        _ => Err(AppError::Validation("传输校验策略无效".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::sftp::TransferPhase;

    #[test]
    fn result_fields_tolerate_missing_values() {
        assert_eq!(result_fields(None), (None, None));
        assert_eq!(
            result_fields(Some(&serde_json::json!({
                "sha256": "abc",
                "location": "downloads/a.txt"
            }))),
            (Some("abc".into()), Some("downloads/a.txt".into()))
        );
    }

    #[test]
    fn phase_maps_to_visible_queue_state() {
        assert_eq!(
            TransferJobStatus::from(TransferPhase::Verifying),
            TransferJobStatus::Verifying
        );
    }

    #[test]
    fn queue_uses_three_global_workers() {
        assert_eq!(QUEUE_WORKERS, 3);
    }
}
