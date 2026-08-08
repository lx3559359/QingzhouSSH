use std::path::PathBuf;

use serde_json::json;
use uuid::Uuid;

use crate::{
    core::{
        sftp::{self, DownloadRequest, TransferOutcome, TransferPhase, UploadRequest},
        ssh::executor::EventSink,
    },
    domain::{
        events::ExecutionEventPayload,
        execution::{
            now_millis, ExecutionDetails, ExecutionFile, ExecutionParameter, ExecutionStatus,
            FinishExecution, NewExecution,
        },
    },
    error::{AppError, AppResult},
    repositories::execution_repository::ExecutionRepository,
    services::{
        event_sink::MonotonicEventSink,
        execution_service::{elapsed, ExecutionRegistry},
        server_connector::ServerConnector,
    },
};

#[derive(Clone)]
pub struct TransferService {
    data_root: PathBuf,
    repository: ExecutionRepository,
    connector: ServerConnector,
    registry: ExecutionRegistry,
}

impl TransferService {
    pub fn new(
        data_root: PathBuf,
        repository: ExecutionRepository,
        connector: ServerConnector,
        registry: ExecutionRegistry,
    ) -> Self {
        Self {
            data_root,
            repository,
            connector,
            registry,
        }
    }

    pub async fn is_idle(&self) -> bool {
        self.registry.is_empty().await
    }

    pub async fn upload<E: EventSink>(
        &self,
        server_id: &str,
        request: UploadRequest,
        events: &mut E,
    ) -> AppResult<ExecutionDetails> {
        sftp::validate_remote_path(&request.remote_path)?;
        let execution_id = self
            .create_execution(
                server_id,
                "transfer.upload",
                vec![
                    parameter("localPath", request.local_path.to_string_lossy().as_ref()),
                    parameter("remotePath", &request.remote_path),
                ],
            )
            .await?;
        let started_at = now_millis();
        self.repository
            .mark_running(execution_id, started_at)
            .await?;
        let cancel = self.registry.register(execution_id).await?;
        let mut sequenced = MonotonicEventSink::new(events);
        sequenced.emit(ExecutionEventPayload::Started {
            execution_id,
            started_at,
        })?;
        sequenced.emit(connecting_progress())?;
        let connected = match self.connector.connect(server_id).await {
            Ok(connected) => connected,
            Err(error) => {
                self.registry.remove(execution_id).await;
                self.finish_error(execution_id, started_at, error, &mut sequenced)
                    .await?;
                return self.details(execution_id).await;
            }
        };
        let result = sftp::upload(
            &connected.session,
            &connected.capabilities,
            &request,
            &mut sequenced,
            cancel,
        )
        .await
        .map_err(|error| redact_transfer_error(error, &connected.redactor));
        self.registry.remove(execution_id).await;
        match result {
            Ok(outcome) => {
                self.finish_success(execution_id, started_at, &outcome, None, &mut sequenced)
                    .await?;
            }
            Err(error) => {
                self.finish_error(execution_id, started_at, error, &mut sequenced)
                    .await?;
            }
        }
        self.details(execution_id).await
    }

    pub async fn download<E: EventSink>(
        &self,
        server_id: &str,
        request: DownloadRequest,
        events: &mut E,
    ) -> AppResult<ExecutionDetails> {
        sftp::validate_remote_path(&request.remote_path)?;
        sftp::download_destination(&self.data_root, &request.suggested_name)?;
        let execution_id = self
            .create_execution(
                server_id,
                "transfer.download",
                vec![
                    parameter("remotePath", &request.remote_path),
                    parameter("suggestedName", &request.suggested_name),
                ],
            )
            .await?;
        let started_at = now_millis();
        self.repository
            .mark_running(execution_id, started_at)
            .await?;
        let cancel = self.registry.register(execution_id).await?;
        let mut sequenced = MonotonicEventSink::new(events);
        sequenced.emit(ExecutionEventPayload::Started {
            execution_id,
            started_at,
        })?;
        sequenced.emit(connecting_progress())?;
        let connected = match self.connector.connect(server_id).await {
            Ok(connected) => connected,
            Err(error) => {
                self.registry.remove(execution_id).await;
                self.finish_error(execution_id, started_at, error, &mut sequenced)
                    .await?;
                return self.details(execution_id).await;
            }
        };
        let result = sftp::download(
            &connected.session,
            &connected.capabilities,
            &self.data_root,
            &request,
            &mut sequenced,
            cancel,
        )
        .await
        .map_err(|error| redact_transfer_error(error, &connected.redactor));
        self.registry.remove(execution_id).await;
        match result {
            Ok(outcome) => {
                let file = ExecutionFile {
                    id: Uuid::new_v4(),
                    relative_path: outcome.location.clone(),
                    purpose: "download".into(),
                    size_bytes: outcome.bytes,
                    sha256: outcome.sha256.clone(),
                };
                self.repository.add_file(execution_id, file.clone()).await?;
                sequenced.emit(ExecutionEventPayload::FileProduced { file: file.clone() })?;
                self.finish_success(
                    execution_id,
                    started_at,
                    &outcome,
                    Some(file),
                    &mut sequenced,
                )
                .await?;
            }
            Err(error) => {
                self.finish_error(execution_id, started_at, error, &mut sequenced)
                    .await?;
            }
        }
        self.details(execution_id).await
    }

    async fn create_execution(
        &self,
        server_id: &str,
        task_id: &str,
        parameters: Vec<ExecutionParameter>,
    ) -> AppResult<Uuid> {
        Ok(self
            .repository
            .create(NewExecution {
                server_id: server_id.into(),
                task_id: task_id.into(),
                task_version: 1,
                category: "transfer".into(),
                parameters,
            })
            .await?
            .id)
    }

    async fn finish_success<E: EventSink>(
        &self,
        execution_id: Uuid,
        started_at: i64,
        outcome: &TransferOutcome,
        file: Option<ExecutionFile>,
        events: &mut MonotonicEventSink<'_, E>,
    ) -> AppResult<()> {
        let finished_at = now_millis();
        let duration_ms = elapsed(started_at, finished_at);
        self.repository
            .finish(FinishExecution {
                id: execution_id,
                status: ExecutionStatus::Succeeded,
                finished_at,
                duration_ms,
                exit_code: None,
                error_category: None,
                error_message: None,
                retryable: false,
                output_summary: Some(format!(
                    "传输 {} 字节，SHA-256 {}",
                    outcome.bytes, outcome.sha256
                )),
                remote_process_group: None,
            })
            .await?;
        events.emit(ExecutionEventPayload::Finished {
            status: ExecutionStatus::Succeeded,
            exit_code: None,
            duration_ms,
            result: Some(json!({
                "bytes": outcome.bytes,
                "sha256": outcome.sha256,
                "location": outcome.location,
                "verificationLevel": outcome.verification_level,
                "remoteHashCompared": outcome.remote_hash_compared,
                "pipelineMaxInFlight": outcome.pipeline_max_in_flight,
                "pipelineMaxBufferedBytes": outcome.pipeline_max_buffered_bytes,
                "file": file,
            })),
        })
    }

    async fn finish_error<E: EventSink>(
        &self,
        execution_id: Uuid,
        started_at: i64,
        error: AppError,
        events: &mut MonotonicEventSink<'_, E>,
    ) -> AppResult<()> {
        let finished_at = now_millis();
        let status = match &error {
            AppError::Cancelled => ExecutionStatus::Cancelled,
            AppError::RemoteStateUncertain(_) => ExecutionStatus::Uncertain,
            _ => ExecutionStatus::Failed,
        };
        let category = error.code().to_string();
        let retryable = error.retryable();
        let message = error.to_string();
        self.repository
            .finish(FinishExecution {
                id: execution_id,
                status,
                finished_at,
                duration_ms: elapsed(started_at, finished_at),
                exit_code: None,
                error_category: Some(category.clone()),
                error_message: Some(message.clone()),
                retryable,
                output_summary: None,
                remote_process_group: None,
            })
            .await?;
        events.emit(ExecutionEventPayload::Failed {
            category,
            message,
            retryable,
        })
    }

    async fn details(&self, execution_id: Uuid) -> AppResult<ExecutionDetails> {
        self.repository
            .get(execution_id)
            .await?
            .ok_or_else(|| AppError::Validation("执行记录不存在".into()))
    }
}

fn parameter(name: &str, value: &str) -> ExecutionParameter {
    ExecutionParameter {
        name: name.into(),
        display_value: value.into(),
        sensitive: false,
    }
}

fn connecting_progress() -> ExecutionEventPayload {
    ExecutionEventPayload::Progress {
        phase: TransferPhase::Connecting,
        transferred: 0,
        total: None,
        percent: None,
        bytes_per_second: None,
        average_bytes_per_second: None,
        eta_seconds: None,
    }
}

fn redact_transfer_error(error: AppError, redactor: &crate::core::redaction::Redactor) -> AppError {
    match error {
        AppError::SshCommand {
            exit_status,
            stderr,
        } => AppError::ssh_command(exit_status, redactor.redact(&stderr)),
        AppError::Transfer(message) => AppError::Transfer(redactor.redact(&message)),
        other => other,
    }
}
