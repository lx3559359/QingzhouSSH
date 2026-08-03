use std::{
    fs::File as StdFile,
    io::Write,
    path::{Path, PathBuf},
};

use uuid::Uuid;

use crate::{
    core::{
        logs::{
            build_search_command, parse_search_output, LogResultPage, LogResultStore,
            LogSearchRequest, StoredLogResults,
        },
        sftp::{download_destination, sha256_local_file},
        ssh::executor::{execute_streaming, CommandRequest, EventSink},
    },
    domain::{
        events::{ExecutionEvent, ExecutionEventPayload},
        execution::{
            now_millis, ExecutionDetails, ExecutionFile, ExecutionParameter, ExecutionStatus,
            FinishExecution, NewExecution,
        },
    },
    error::{AppError, AppResult},
    repositories::execution_repository::ExecutionRepository,
    services::{
        event_sink::MonotonicEventSink,
        execution_service::{elapsed, relative_to, ExecutionRegistry},
        server_connector::ServerConnector,
    },
};

const LOG_OUTPUT_LIMIT: u64 = 32 * 1024 * 1024;

#[derive(Clone)]
pub struct LogService {
    data_root: PathBuf,
    repository: ExecutionRepository,
    connector: ServerConnector,
    registry: ExecutionRegistry,
    store: LogResultStore,
}

impl LogService {
    pub fn new(
        data_root: PathBuf,
        repository: ExecutionRepository,
        connector: ServerConnector,
        registry: ExecutionRegistry,
    ) -> Self {
        Self {
            store: LogResultStore::new(&data_root),
            data_root,
            repository,
            connector,
            registry,
        }
    }

    pub async fn search<E: EventSink>(
        &self,
        server_id: &str,
        request: LogSearchRequest,
        events: &mut E,
    ) -> AppResult<ExecutionDetails> {
        request.validate()?;
        let execution_id = self
            .repository
            .create(NewExecution {
                server_id: server_id.into(),
                task_id: "logs.search".into(),
                task_version: 1,
                category: "logs".into(),
                parameters: search_parameters(&request),
            })
            .await?
            .id;
        let started_at = now_millis();
        self.repository
            .mark_running(execution_id, started_at)
            .await?;
        let cancel = self.registry.register(execution_id).await?;
        let mut sequenced = MonotonicEventSink::new(events);
        let connected = match self.connector.connect(server_id).await {
            Ok(connected) => connected,
            Err(error) => {
                self.registry.remove(execution_id).await;
                self.finish_error(execution_id, started_at, error, &mut sequenced)
                    .await?;
                return self.details(execution_id).await;
            }
        };
        let command = match build_search_command(&request, &connected.capabilities) {
            Ok(command) => command,
            Err(error) => {
                connected.session.disconnect().await;
                self.registry.remove(execution_id).await;
                self.finish_error(execution_id, started_at, error, &mut sequenced)
                    .await?;
                return self.details(execution_id).await;
            }
        };
        let directory = self.search_directory(execution_id);
        tokio::fs::create_dir_all(&directory).await?;
        let capture_path = directory.join("capture.tmp");
        let output_path = self
            .data_root
            .join("logs/executions")
            .join(format!("{execution_id}.log"));
        let capture_file = StdFile::create(&capture_path)?;
        let outcome = {
            let mut capture = CaptureEventSink::new(&mut sequenced, capture_file);
            let outcome = execute_streaming(
                &connected.session,
                CommandRequest {
                    execution_id,
                    command,
                    timeout: connected.session.timeout(),
                    max_output_bytes: LOG_OUTPUT_LIMIT,
                },
                &output_path,
                &connected.redactor,
                &mut capture,
                cancel,
            )
            .await;
            capture.flush()?;
            outcome
        };
        connected.session.disconnect().await;
        self.registry.remove(execution_id).await;
        let result = match outcome {
            Ok(outcome) if outcome.exit_status == 0 => {
                let captured = tokio::fs::read_to_string(&capture_path).await?;
                let matches = parse_search_output(&request, &captured, &connected.redactor)?;
                let stored = self.store.write(execution_id, &matches).await?;
                self.record_files(execution_id, &stored, &output_path)
                    .await?;
                self.finish_success(execution_id, started_at, &stored, &mut sequenced)
                    .await
            }
            Ok(outcome) => {
                self.finish_error(
                    execution_id,
                    started_at,
                    AppError::ssh_command(outcome.exit_status, "远程日志检索返回非零退出码".into()),
                    &mut sequenced,
                )
                .await
            }
            Err(error) => {
                self.finish_error(execution_id, started_at, error, &mut sequenced)
                    .await
            }
        };
        if capture_path.exists() {
            let _ = tokio::fs::remove_file(capture_path).await;
        }
        result?;
        self.details(execution_id).await
    }

    pub async fn read_page(
        &self,
        execution_id: Uuid,
        cursor: Option<&str>,
        page_size: usize,
    ) -> AppResult<LogResultPage> {
        self.store.read_page(execution_id, cursor, page_size).await
    }

    pub async fn download_result(
        &self,
        execution_id: Uuid,
        suggested_name: &str,
    ) -> AppResult<String> {
        let source = self.store.text_path(execution_id);
        if !source.is_file() {
            return Err(AppError::Validation("日志检索结果不存在".into()));
        }
        let destination = download_destination(&self.data_root, suggested_name)?;
        if destination.exists() {
            return Err(AppError::Validation("下载目标已经存在".into()));
        }
        tokio::fs::copy(&source, &destination).await?;
        relative_to(&self.data_root, &destination)
    }

    async fn record_files(
        &self,
        execution_id: Uuid,
        stored: &StoredLogResults,
        output_path: &Path,
    ) -> AppResult<()> {
        for (relative_path, purpose, sha256) in [
            (
                &stored.jsonl_relative_path,
                "log_results_jsonl",
                &stored.jsonl_sha256,
            ),
            (
                &stored.text_relative_path,
                "log_results_text",
                &stored.text_sha256,
            ),
        ] {
            let path = self.data_root.join(relative_path);
            self.repository
                .add_file(
                    execution_id,
                    ExecutionFile {
                        id: Uuid::new_v4(),
                        relative_path: relative_path.clone(),
                        purpose: purpose.into(),
                        size_bytes: tokio::fs::metadata(path).await?.len(),
                        sha256: sha256.clone(),
                    },
                )
                .await?;
        }
        if output_path.exists() {
            self.repository
                .add_file(
                    execution_id,
                    ExecutionFile {
                        id: Uuid::new_v4(),
                        relative_path: relative_to(&self.data_root, output_path)?,
                        purpose: "execution_log".into(),
                        size_bytes: tokio::fs::metadata(output_path).await?.len(),
                        sha256: sha256_local_file(output_path).await?,
                    },
                )
                .await?;
        }
        Ok(())
    }

    async fn finish_success<E: EventSink>(
        &self,
        execution_id: Uuid,
        started_at: i64,
        stored: &StoredLogResults,
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
                exit_code: Some(0),
                error_category: None,
                error_message: None,
                retryable: false,
                output_summary: Some(format!("{} 条日志记录", stored.count)),
                remote_process_group: None,
            })
            .await?;
        events.emit(ExecutionEventPayload::Finished {
            status: ExecutionStatus::Succeeded,
            exit_code: Some(0),
            duration_ms,
            result: Some(
                serde_json::to_value(stored)
                    .map_err(|error| AppError::Serialization(error.to_string()))?,
            ),
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

    fn search_directory(&self, execution_id: Uuid) -> PathBuf {
        self.data_root
            .join("logs")
            .join("searches")
            .join(execution_id.to_string())
    }
}

struct CaptureEventSink<'capture, 'inner, E: EventSink> {
    inner: &'capture mut MonotonicEventSink<'inner, E>,
    stdout: StdFile,
}

impl<'capture, 'inner, E: EventSink> CaptureEventSink<'capture, 'inner, E> {
    fn new(inner: &'capture mut MonotonicEventSink<'inner, E>, stdout: StdFile) -> Self {
        Self { inner, stdout }
    }

    fn flush(&mut self) -> AppResult<()> {
        self.stdout.flush()?;
        self.stdout.sync_all()?;
        Ok(())
    }
}

impl<E: EventSink> EventSink for CaptureEventSink<'_, '_, E> {
    fn send(&mut self, event: ExecutionEvent) -> AppResult<()> {
        if let ExecutionEventPayload::Stdout { text, .. } = &event.payload {
            self.stdout.write_all(text.as_bytes())?;
        }
        self.inner.send(event)
    }
}

fn search_parameters(request: &LogSearchRequest) -> Vec<ExecutionParameter> {
    vec![
        parameter("path", &request.path),
        parameter("keyword", &request.keyword),
        parameter("caseSensitive", &request.case_sensitive.to_string()),
        parameter("contextLines", &request.context_lines.to_string()),
        parameter("limit", &request.limit.to_string()),
    ]
}

fn parameter(name: &str, value: &str) -> ExecutionParameter {
    ExecutionParameter {
        name: name.into(),
        display_value: value.into(),
        sensitive: false,
    }
}
