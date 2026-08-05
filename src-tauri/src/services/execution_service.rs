use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    core::{
        sftp::sha256_local_file,
        ssh::executor::{execute_streaming, CommandRequest, EventSink},
        tasks::{
            built_in_catalog, render_command, select_implementation, validate_parameters,
            RenderedTaskStep, RiskLevel, TaskDefinition, ValidatedParameters,
        },
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
    services::{event_sink::MonotonicEventSink, server_connector::ServerConnector},
};

const DEFAULT_OUTPUT_LIMIT: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskExecutionRequest {
    pub task_id: String,
    pub parameters: Value,
    #[serde(default)]
    pub dangerous_confirmed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAvailability {
    pub definition: TaskDefinition,
    pub compatible: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomExecutionMode {
    Command,
    Script,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomExecutionRequest {
    pub mode: CustomExecutionMode,
    pub content: String,
    pub timeout_seconds: u64,
    pub dangerous_confirmed: bool,
}

impl CustomExecutionRequest {
    pub fn render(&self) -> AppResult<String> {
        if !self.dangerous_confirmed {
            return Err(AppError::Validation("高级命令或脚本必须二次确认".into()));
        }
        if self.content.trim().is_empty()
            || self.content.contains('\0')
            || self.content.len() > 1024 * 1024
        {
            return Err(AppError::Validation("高级命令或脚本内容无效".into()));
        }
        if !(1..=3_600).contains(&self.timeout_seconds) {
            return Err(AppError::Validation(
                "高级命令超时必须在 1 到 3600 秒之间".into(),
            ));
        }
        match self.mode {
            CustomExecutionMode::Command => Ok(self.content.clone()),
            CustomExecutionMode::Script => {
                let delimiter = format!("__QZ_SCRIPT_{}__", Uuid::new_v4().simple());
                Ok(format!(
                    "sh -s <<'{delimiter}'\n{}\n{delimiter}",
                    self.content
                ))
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct ExecutionRegistry {
    tokens: Arc<Mutex<HashMap<Uuid, CancellationToken>>>,
}

impl ExecutionRegistry {
    pub async fn register(&self, id: Uuid) -> AppResult<CancellationToken> {
        self.register_token(id, CancellationToken::new()).await
    }

    pub async fn register_child(
        &self,
        id: Uuid,
        parent: &CancellationToken,
    ) -> AppResult<CancellationToken> {
        self.register_token(id, parent.child_token()).await
    }

    async fn register_token(
        &self,
        id: Uuid,
        token: CancellationToken,
    ) -> AppResult<CancellationToken> {
        let mut tokens = self.tokens.lock().await;
        if tokens.contains_key(&id) {
            return Err(AppError::Validation("执行标识已经在运行".into()));
        }
        tokens.insert(id, token.clone());
        Ok(token)
    }

    pub async fn cancel(&self, id: Uuid) -> AppResult<()> {
        let tokens = self.tokens.lock().await;
        let token = tokens
            .get(&id)
            .ok_or_else(|| AppError::Validation("执行不存在或已经结束".into()))?;
        token.cancel();
        Ok(())
    }

    pub async fn remove(&self, id: Uuid) {
        self.tokens.lock().await.remove(&id);
    }

    pub async fn contains(&self, id: Uuid) -> bool {
        self.tokens.lock().await.contains_key(&id)
    }
}

#[derive(Clone)]
pub struct ExecutionService {
    data_root: PathBuf,
    repository: ExecutionRepository,
    connector: ServerConnector,
    registry: ExecutionRegistry,
}

impl ExecutionService {
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

    pub async fn list_task_definitions(&self, server_id: &str) -> AppResult<Vec<TaskAvailability>> {
        let connected = self.connector.connect(server_id).await?;
        let capabilities = connected.capabilities.clone();
        connected.session.disconnect().await;
        Ok(built_in_catalog()
            .into_iter()
            .map(
                |definition| match select_implementation(&definition, &capabilities) {
                    Ok(_) => TaskAvailability {
                        definition,
                        compatible: true,
                        reason: None,
                    },
                    Err(error) => TaskAvailability {
                        definition,
                        compatible: false,
                        reason: Some(error.to_string()),
                    },
                },
            )
            .collect())
    }

    pub async fn execute_task<E: EventSink>(
        &self,
        server_id: &str,
        request: TaskExecutionRequest,
        events: &mut E,
    ) -> AppResult<ExecutionDetails> {
        let definition = built_in_catalog()
            .into_iter()
            .find(|definition| definition.id == request.task_id)
            .ok_or_else(|| AppError::Validation("快捷任务不存在".into()))?;
        reject_legacy_dangerous_task(&definition)?;
        let parameters = validate_parameters(&definition, &request.parameters)?;
        let execution = self
            .repository
            .create(NewExecution {
                server_id: server_id.into(),
                task_id: definition.id.clone(),
                task_version: definition.version,
                category: definition.category.as_str().into(),
                parameters: history_parameters(&parameters),
            })
            .await?;
        self.run_task(execution.id, definition, parameters, events)
            .await
    }

    pub async fn execute_custom<E: EventSink>(
        &self,
        server_id: &str,
        request: CustomExecutionRequest,
        events: &mut E,
    ) -> AppResult<ExecutionDetails> {
        let command = request.render()?;
        let mode = match request.mode {
            CustomExecutionMode::Command => "command",
            CustomExecutionMode::Script => "script",
        };
        let execution = self
            .repository
            .create(NewExecution {
                server_id: server_id.into(),
                task_id: format!("advanced.{mode}"),
                task_version: 1,
                category: "advanced".into(),
                parameters: vec![
                    ExecutionParameter {
                        name: "mode".into(),
                        display_value: mode.into(),
                        sensitive: false,
                    },
                    ExecutionParameter {
                        name: "content".into(),
                        display_value: "[REDACTED]".into(),
                        sensitive: true,
                    },
                    ExecutionParameter {
                        name: "timeoutSeconds".into(),
                        display_value: request.timeout_seconds.to_string(),
                        sensitive: false,
                    },
                ],
            })
            .await?;
        self.run_command(
            execution.id,
            command,
            Duration::from_secs(request.timeout_seconds),
            events,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn execute_planned_step_with_cancel<E: EventSink>(
        &self,
        server_id: &str,
        task_id: &str,
        task_version: i32,
        category: &str,
        step: &RenderedTaskStep,
        parameters: &[ExecutionParameter],
        events: &mut E,
        cancel: &CancellationToken,
    ) -> AppResult<ExecutionDetails> {
        self.execute_planned_step_inner(
            server_id,
            task_id,
            task_version,
            category,
            step,
            parameters,
            events,
            Some(cancel),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_planned_step_inner<E: EventSink>(
        &self,
        server_id: &str,
        task_id: &str,
        task_version: i32,
        category: &str,
        step: &RenderedTaskStep,
        parameters: &[ExecutionParameter],
        events: &mut E,
        cancel: Option<&CancellationToken>,
    ) -> AppResult<ExecutionDetails> {
        let execution = self
            .repository
            .create(NewExecution {
                server_id: server_id.into(),
                task_id: task_id.into(),
                task_version,
                category: category.into(),
                parameters: parameters.to_vec(),
            })
            .await?;
        self.run_command_with_limit_and_cancel(
            execution.id,
            step.command.clone(),
            Duration::from_secs(step.timeout_seconds),
            step.output_limit_bytes,
            events,
            cancel,
        )
        .await
    }

    pub async fn cancel(&self, execution_id: Uuid) -> AppResult<()> {
        self.registry.cancel(execution_id).await
    }

    pub async fn list(
        &self,
        filter: crate::domain::execution::ExecutionFilter,
    ) -> AppResult<Vec<crate::domain::execution::ExecutionRecord>> {
        self.repository.list(filter).await
    }

    pub async fn get(&self, id: Uuid) -> AppResult<Option<ExecutionDetails>> {
        self.repository.get(id).await
    }

    async fn run_task<E: EventSink>(
        &self,
        execution_id: Uuid,
        definition: TaskDefinition,
        parameters: ValidatedParameters,
        events: &mut E,
    ) -> AppResult<ExecutionDetails> {
        let server_id = self
            .repository
            .get(execution_id)
            .await?
            .ok_or_else(|| AppError::Validation("执行记录不存在".into()))?
            .record
            .server_id;
        let connected = match self.connector.connect(&server_id).await {
            Ok(connected) => connected,
            Err(error) => return self.fail_before_run(execution_id, error, events).await,
        };
        let implementation = match select_implementation(&definition, &connected.capabilities) {
            Ok(implementation) => implementation,
            Err(error) => {
                connected.session.disconnect().await;
                return self.fail_before_run(execution_id, error, events).await;
            }
        };
        let command = match render_command(implementation, &parameters) {
            Ok(command) => command,
            Err(error) => {
                connected.session.disconnect().await;
                return self.fail_before_run(execution_id, error, events).await;
            }
        };
        self.run_connected_command(
            execution_id,
            command,
            connected,
            Duration::from_secs(60),
            DEFAULT_OUTPUT_LIMIT,
            events,
            None,
        )
        .await
    }

    async fn run_command<E: EventSink>(
        &self,
        execution_id: Uuid,
        command: String,
        timeout: Duration,
        events: &mut E,
    ) -> AppResult<ExecutionDetails> {
        self.run_command_with_limit(execution_id, command, timeout, DEFAULT_OUTPUT_LIMIT, events)
            .await
    }

    async fn run_command_with_limit<E: EventSink>(
        &self,
        execution_id: Uuid,
        command: String,
        timeout: Duration,
        max_output_bytes: u64,
        events: &mut E,
    ) -> AppResult<ExecutionDetails> {
        self.run_command_with_limit_and_cancel(
            execution_id,
            command,
            timeout,
            max_output_bytes,
            events,
            None,
        )
        .await
    }

    async fn run_command_with_limit_and_cancel<E: EventSink>(
        &self,
        execution_id: Uuid,
        command: String,
        timeout: Duration,
        max_output_bytes: u64,
        events: &mut E,
        cancel: Option<&CancellationToken>,
    ) -> AppResult<ExecutionDetails> {
        let server_id = self
            .repository
            .get(execution_id)
            .await?
            .ok_or_else(|| AppError::Validation("执行记录不存在".into()))?
            .record
            .server_id;
        let connected_result = match cancel {
            Some(cancel) => tokio::select! {
                _ = cancel.cancelled() => Err(AppError::Cancelled),
                result = self.connector.connect(&server_id) => result,
            },
            None => self.connector.connect(&server_id).await,
        };
        let connected = match connected_result {
            Ok(connected) => connected,
            Err(error) => return self.fail_before_run(execution_id, error, events).await,
        };
        self.run_connected_command(
            execution_id,
            command,
            connected,
            timeout,
            max_output_bytes,
            events,
            cancel,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_connected_command<E: EventSink>(
        &self,
        execution_id: Uuid,
        command: String,
        connected: crate::services::server_connector::ConnectedServer,
        timeout: Duration,
        max_output_bytes: u64,
        events: &mut E,
        parent_cancel: Option<&CancellationToken>,
    ) -> AppResult<ExecutionDetails> {
        let started_at = now_millis();
        self.repository
            .mark_running(execution_id, started_at)
            .await?;
        let cancel = match parent_cancel {
            Some(parent) => self.registry.register_child(execution_id, parent).await?,
            None => self.registry.register(execution_id).await?,
        };
        let output_path = self.execution_log_path(execution_id);
        let mut sequenced = MonotonicEventSink::new(events);
        let outcome = execute_streaming(
            &connected.session,
            CommandRequest {
                execution_id,
                command,
                timeout,
                max_output_bytes,
            },
            &output_path,
            &connected.redactor,
            &mut sequenced,
            cancel,
        )
        .await;
        connected.session.disconnect().await;
        self.registry.remove(execution_id).await;

        let result = match outcome {
            Ok(outcome) if outcome.exit_status == 0 => {
                if output_path.exists() {
                    self.record_output_file(execution_id, &output_path).await?;
                }
                let finished_at = now_millis();
                self.repository
                    .finish(FinishExecution {
                        id: execution_id,
                        status: ExecutionStatus::Succeeded,
                        finished_at,
                        duration_ms: elapsed(started_at, finished_at),
                        exit_code: Some(0),
                        error_category: None,
                        error_message: None,
                        retryable: false,
                        output_summary: Some(format!(
                            "stdout {} 字节，stderr {} 字节",
                            outcome.stdout_bytes, outcome.stderr_bytes
                        )),
                        remote_process_group: None,
                    })
                    .await?;
                sequenced.emit(ExecutionEventPayload::Finished {
                    status: ExecutionStatus::Succeeded,
                    exit_code: Some(0),
                    duration_ms: elapsed(started_at, finished_at),
                    result: None,
                })?;
                Ok(())
            }
            Ok(outcome) => {
                let error =
                    AppError::ssh_command(outcome.exit_status, "远程命令返回非零退出码".into());
                self.finish_error(execution_id, started_at, error, &mut sequenced)
                    .await
            }
            Err(error) => {
                self.finish_error(execution_id, started_at, error, &mut sequenced)
                    .await
            }
        };
        result?;
        self.repository
            .get(execution_id)
            .await?
            .ok_or_else(|| AppError::Validation("执行记录不存在".into()))
    }

    async fn fail_before_run<E: EventSink>(
        &self,
        execution_id: Uuid,
        error: AppError,
        events: &mut E,
    ) -> AppResult<ExecutionDetails> {
        let started_at = now_millis();
        self.repository
            .mark_running(execution_id, started_at)
            .await?;
        let mut sequenced = MonotonicEventSink::new(events);
        self.finish_error(execution_id, started_at, error, &mut sequenced)
            .await?;
        self.repository
            .get(execution_id)
            .await?
            .ok_or_else(|| AppError::Validation("执行记录不存在".into()))
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
                exit_code: match &error {
                    AppError::SshCommand { exit_status, .. } => Some(*exit_status),
                    _ => None,
                },
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

    async fn record_output_file(
        &self,
        execution_id: Uuid,
        path: &std::path::Path,
    ) -> AppResult<()> {
        let metadata = tokio::fs::metadata(path).await?;
        self.repository
            .add_file(
                execution_id,
                ExecutionFile {
                    id: Uuid::new_v4(),
                    relative_path: relative_to(&self.data_root, path)?,
                    purpose: "execution_log".into(),
                    size_bytes: metadata.len(),
                    sha256: sha256_local_file(path).await?,
                },
            )
            .await
    }

    fn execution_log_path(&self, execution_id: Uuid) -> PathBuf {
        self.data_root
            .join("logs")
            .join("executions")
            .join(format!("{execution_id}.log"))
    }
}

fn history_parameters(parameters: &ValidatedParameters) -> Vec<ExecutionParameter> {
    parameters
        .iter()
        .map(|(name, parameter)| ExecutionParameter {
            name: name.clone(),
            display_value: if parameter.sensitive {
                "[REDACTED]".into()
            } else if let Some(value) = parameter.value.as_str() {
                value.into()
            } else {
                parameter.value.to_string()
            },
            sensitive: parameter.sensitive,
        })
        .collect()
}

fn reject_legacy_dangerous_task(definition: &TaskDefinition) -> AppResult<()> {
    if definition.risk_level == RiskLevel::Dangerous {
        return Err(AppError::Security(
            "危险任务必须通过运维中心执行，以启用预演、备份、验证和回滚保护".into(),
        ));
    }
    Ok(())
}

pub(crate) fn relative_to(root: &std::path::Path, path: &std::path::Path) -> AppResult<String> {
    Ok(path
        .strip_prefix(root)
        .map_err(|_| AppError::Security("产生文件路径逃逸数据根目录".into()))?
        .to_string_lossy()
        .replace('\\', "/"))
}

pub(crate) fn elapsed(started_at: i64, finished_at: i64) -> u64 {
    u64::try_from(finished_at.saturating_sub(started_at)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn registered_child_execution_inherits_parent_cancellation() {
        let registry = ExecutionRegistry::default();
        let parent = CancellationToken::new();
        let child = registry
            .register_child(Uuid::new_v4(), &parent)
            .await
            .unwrap();
        assert!(!child.is_cancelled());
        parent.cancel();
        assert!(child.is_cancelled());
    }

    #[test]
    fn legacy_task_execution_cannot_bypass_dangerous_operation_recovery() {
        let dangerous = built_in_catalog()
            .into_iter()
            .find(|task| task.risk_level == RiskLevel::Dangerous)
            .unwrap();
        let safe = built_in_catalog()
            .into_iter()
            .find(|task| task.risk_level == RiskLevel::Safe)
            .unwrap();
        assert_eq!(
            reject_legacy_dangerous_task(&dangerous).unwrap_err().code(),
            "security"
        );
        reject_legacy_dangerous_task(&safe).unwrap();
    }
}
