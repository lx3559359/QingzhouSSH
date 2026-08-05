use std::{collections::HashMap, path::PathBuf, sync::Arc};

use serde::Serialize;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    core::{
        scripts::{
            environment::render_script_launcher,
            package::{export_script_package, import_script_package, ScriptPackageExport},
            validation::{
                scan_script_body, validate_script_parameter_values, ScriptScanWarning,
                PERSONAL_SCRIPT_AUTOMATIC_ROLLBACK_AVAILABLE, PERSONAL_SCRIPT_RISK,
            },
        },
        ssh::executor::EventSink,
        tasks::{ParameterDefinition, RiskLevel, ValidatedParameters},
    },
    domain::{
        execution::{now_millis, ExecutionDetails, ExecutionParameter, ExecutionStatus},
        operation::{NewOperationRun, OperationStatus},
        script::{
            NewPersonalScript, NewScriptRunReference, NewScriptVersion, ScriptDetails,
            ScriptListFilter, ScriptMetadataUpdate, ScriptSummary, ScriptVersion,
        },
    },
    error::{AppError, AppResult},
    repositories::{
        operation_repository::OperationRepository, script_repository::ScriptRepository,
    },
    services::execution_service::ExecutionService,
};

const PREVIEW_TTL_MILLIS: i64 = 5 * 60 * 1000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptRunPreview {
    pub preview_id: Uuid,
    pub confirmation_token: Uuid,
    pub expires_at: i64,
    pub server_id: String,
    pub script_definition_id: Uuid,
    pub script_version_id: Uuid,
    pub script_version_number: u32,
    pub title: String,
    pub risk_level: RiskLevel,
    pub automatic_rollback_available: bool,
    pub warning: String,
    pub line_count: usize,
    pub character_count: usize,
    pub body_sha256: String,
    pub parameter_names: Vec<String>,
    pub scan_warnings: Vec<ScriptScanWarning>,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptRunResult {
    pub operation_run_id: Uuid,
    pub script_definition_id: Uuid,
    pub script_version_id: Uuid,
    pub execution: ExecutionDetails,
}

#[derive(Debug, Clone)]
struct PendingScriptPreview {
    confirmation_token: Uuid,
    expires_at: i64,
    server_id: String,
    definition_id: Uuid,
    version_id: Uuid,
    version_number: u32,
    parameter_values: Value,
}

#[derive(Clone)]
pub struct ScriptService {
    data_root: PathBuf,
    repository: ScriptRepository,
    operations: OperationRepository,
    executions: ExecutionService,
    previews: Arc<Mutex<HashMap<Uuid, PendingScriptPreview>>>,
    active: Arc<Mutex<HashMap<Uuid, CancellationToken>>>,
}

impl ScriptService {
    pub fn new(
        data_root: PathBuf,
        repository: ScriptRepository,
        operations: OperationRepository,
        executions: ExecutionService,
    ) -> Self {
        Self {
            data_root,
            repository,
            operations,
            executions,
            previews: Arc::new(Mutex::new(HashMap::new())),
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn create(&self, draft: NewPersonalScript) -> AppResult<ScriptDetails> {
        self.repository.create(draft).await
    }

    pub async fn save_version(
        &self,
        definition_id: Uuid,
        draft: NewScriptVersion,
    ) -> AppResult<ScriptVersion> {
        self.repository.save_version(definition_id, draft).await
    }

    pub async fn get_for_editor(&self, id: Uuid) -> AppResult<Option<ScriptDetails>> {
        self.repository.get_for_editor(id).await
    }

    pub async fn list_versions(&self, id: Uuid) -> AppResult<Vec<ScriptVersion>> {
        self.repository.list_versions(id).await
    }

    pub async fn list(&self, filter: ScriptListFilter) -> AppResult<Vec<ScriptSummary>> {
        self.repository.list(filter).await
    }

    pub async fn update_metadata(&self, id: Uuid, update: ScriptMetadataUpdate) -> AppResult<()> {
        self.repository.update_metadata(id, update).await
    }

    pub async fn set_favorite(&self, id: Uuid, favorite: bool) -> AppResult<()> {
        self.repository.set_favorite(id, favorite).await
    }

    pub async fn set_enabled(&self, id: Uuid, enabled: bool) -> AppResult<()> {
        self.repository.set_enabled(id, enabled).await
    }

    pub async fn delete(&self, id: Uuid) -> AppResult<()> {
        self.repository.soft_delete(id).await
    }

    pub async fn copy(&self, id: Uuid) -> AppResult<ScriptDetails> {
        let details = self
            .repository
            .get_for_editor(id)
            .await?
            .ok_or_else(|| AppError::Validation("脚本不存在或已删除".into()))?;
        let suffix = " 副本";
        let title = details
            .definition
            .title
            .chars()
            .take(80 - suffix.chars().count())
            .collect::<String>();
        self.repository
            .create(NewPersonalScript {
                title: format!("{title}{suffix}"),
                category: details.definition.category,
                tags: details.definition.tags,
                is_favorite: false,
                is_enabled: false,
                version: NewScriptVersion {
                    body: details.active_version.body,
                    parameters: details.active_version.parameters,
                    scan_summary: Value::Null,
                    timeout_seconds: details.active_version.timeout_seconds,
                },
            })
            .await
    }

    pub async fn import(&self, bytes: &[u8]) -> AppResult<ScriptDetails> {
        self.repository.create(import_script_package(bytes)?).await
    }

    pub async fn export(&self, id: Uuid) -> AppResult<ScriptPackageExport> {
        let details = self
            .repository
            .get_for_editor(id)
            .await?
            .ok_or_else(|| AppError::Validation("脚本不存在或已删除".into()))?;
        export_script_package(&self.data_root, &details).await
    }

    pub async fn preview_run(
        &self,
        definition_id: Uuid,
        server_id: &str,
        parameter_values: Value,
    ) -> AppResult<ScriptRunPreview> {
        let details = self.require_enabled(definition_id).await?;
        let parameters = parse_parameters(&details.active_version)?;
        let validated = validate_script_parameter_values(&parameters, &parameter_values)?;
        let scan = scan_script_body(&details.active_version.body)?;
        let version_number = i32::try_from(details.active_version.version_number)
            .map_err(|_| AppError::Validation("脚本版本超出运行范围".into()))?;
        let operation = self
            .operations
            .create(NewOperationRun {
                server_id: server_id.into(),
                task_id: "script.personal".into(),
                task_version: version_number,
                risk_level: PERSONAL_SCRIPT_RISK,
                parameters_summary: operation_parameter_summary(
                    details.definition.id,
                    details.active_version.id,
                    &details.active_version.body_sha256,
                    details.active_version.timeout_seconds,
                    &validated,
                ),
            })
            .await?;
        self.operations
            .transition(operation.id, OperationStatus::Preflighting)
            .await?;
        self.operations
            .transition(operation.id, OperationStatus::PreviewReady)
            .await?;
        self.operations
            .transition(operation.id, OperationStatus::WaitingConfirmation)
            .await?;

        let confirmation_token = Uuid::new_v4();
        let expires_at = now_millis().saturating_add(PREVIEW_TTL_MILLIS);
        self.previews.lock().await.insert(
            operation.id,
            PendingScriptPreview {
                confirmation_token,
                expires_at,
                server_id: server_id.into(),
                definition_id,
                version_id: details.active_version.id,
                version_number: details.active_version.version_number,
                parameter_values,
            },
        );
        Ok(ScriptRunPreview {
            preview_id: operation.id,
            confirmation_token,
            expires_at,
            server_id: server_id.into(),
            script_definition_id: definition_id,
            script_version_id: details.active_version.id,
            script_version_number: details.active_version.version_number,
            title: details.definition.title,
            risk_level: PERSONAL_SCRIPT_RISK,
            automatic_rollback_available: PERSONAL_SCRIPT_AUTOMATIC_ROLLBACK_AVAILABLE,
            warning: "不可自动回滚：此脚本造成的修改无法由客户端自动恢复，请确认目标服务器和参数后再运行。"
                .into(),
            line_count: scan.line_count,
            character_count: scan.character_count,
            body_sha256: scan.body_sha256,
            parameter_names: parameters
                .into_iter()
                .map(|parameter| parameter.name)
                .collect(),
            scan_warnings: scan.warnings,
            timeout_seconds: details.active_version.timeout_seconds,
        })
    }

    pub async fn confirm_run<E: EventSink>(
        &self,
        preview_id: Uuid,
        confirmation_token: Uuid,
        events: &mut E,
    ) -> AppResult<ScriptRunResult> {
        let pending = self.consume_preview(preview_id, confirmation_token).await?;
        if let Err(error) = self.require_enabled(pending.definition_id).await {
            let _ = self
                .operations
                .transition(preview_id, OperationStatus::Cancelled)
                .await;
            return Err(error);
        }
        let version = self
            .repository
            .get_version(pending.definition_id, pending.version_number)
            .await?;
        if version.id != pending.version_id {
            let _ = self
                .operations
                .transition(preview_id, OperationStatus::Cancelled)
                .await;
            return Err(AppError::Integrity("预演锁定的脚本版本不一致".into()));
        }
        let parameters = parse_parameters(&version)?;
        let validated = validate_script_parameter_values(&parameters, &pending.parameter_values)?;
        let launcher = render_script_launcher(&version.body, &validated)?;
        let history = execution_history_parameters(
            pending.definition_id,
            version.id,
            &version.body_sha256,
            version.timeout_seconds,
            &validated,
        );
        let cancel = CancellationToken::new();
        let version_number = i32::try_from(version.version_number)
            .map_err(|_| AppError::Validation("脚本版本超出运行范围".into()))?;
        self.operations
            .start_confirmed_unrecoverable_personal_script(preview_id)
            .await?;
        if let Err(error) = self
            .repository
            .record_run(NewScriptRunReference {
                definition_id: pending.definition_id,
                version_id: version.id,
                operation_run_id: preview_id,
            })
            .await
        {
            let _ = self
                .operations
                .transition(preview_id, OperationStatus::Failed)
                .await;
            return Err(error);
        }
        self.active.lock().await.insert(preview_id, cancel.clone());
        let execution = self
            .executions
            .execute_personal_script_with_cancel(
                &pending.server_id,
                version_number,
                launcher,
                version.timeout_seconds,
                history,
                events,
                &cancel,
            )
            .await;
        self.active.lock().await.remove(&preview_id);
        let execution = match execution {
            Ok(details) => details,
            Err(error) => {
                let _ = self
                    .operations
                    .transition(preview_id, OperationStatus::Failed)
                    .await;
                return Err(error);
            }
        };
        self.operations
            .transition(preview_id, operation_status(execution.record.status))
            .await?;
        Ok(ScriptRunResult {
            operation_run_id: preview_id,
            script_definition_id: pending.definition_id,
            script_version_id: version.id,
            execution,
        })
    }

    pub async fn cancel_run(&self, operation_run_id: Uuid) -> AppResult<()> {
        if self
            .previews
            .lock()
            .await
            .remove(&operation_run_id)
            .is_some()
        {
            return self
                .operations
                .transition(operation_run_id, OperationStatus::Cancelled)
                .await;
        }
        let token = self.active.lock().await.get(&operation_run_id).cloned();
        let token = token
            .ok_or_else(|| AppError::Validation("脚本运行不存在、已经结束或无法取消".into()))?;
        token.cancel();
        Ok(())
    }

    async fn consume_preview(
        &self,
        preview_id: Uuid,
        confirmation_token: Uuid,
    ) -> AppResult<PendingScriptPreview> {
        let mut previews = self.previews.lock().await;
        let Some(pending) = previews.get(&preview_id) else {
            return Err(AppError::ScriptConfirmationRequired(
                "预演不存在、已经使用或应用已重新启动".into(),
            ));
        };
        if pending.confirmation_token != confirmation_token {
            return Err(AppError::ScriptConfirmationRequired(
                "确认令牌与本次预演不匹配".into(),
            ));
        }
        if pending.expires_at < now_millis() {
            previews.remove(&preview_id);
            drop(previews);
            let _ = self
                .operations
                .transition(preview_id, OperationStatus::Cancelled)
                .await;
            return Err(AppError::ScriptPreviewExpired(
                "请重新预演脚本并再次确认".into(),
            ));
        }
        previews
            .remove(&preview_id)
            .ok_or_else(|| AppError::ScriptConfirmationRequired("本次预演已经被使用".into()))
    }

    async fn require_enabled(&self, id: Uuid) -> AppResult<ScriptDetails> {
        let details = self
            .repository
            .get_for_editor(id)
            .await?
            .ok_or_else(|| AppError::Validation("脚本不存在或已删除".into()))?;
        if !details.definition.is_enabled {
            return Err(AppError::Validation(
                "脚本尚未启用，请检查内容后先启用再运行".into(),
            ));
        }
        Ok(details)
    }
}

fn parse_parameters(version: &ScriptVersion) -> AppResult<Vec<ParameterDefinition>> {
    serde_json::from_value(version.parameters.clone())
        .map_err(|_| AppError::Integrity("已保存的脚本参数定义无效".into()))
}

fn execution_history_parameters(
    definition_id: Uuid,
    version_id: Uuid,
    body_sha256: &str,
    timeout_seconds: u64,
    parameters: &ValidatedParameters,
) -> Vec<ExecutionParameter> {
    let mut values = vec![
        public_history("scriptDefinitionId", definition_id.to_string()),
        public_history("scriptVersionId", version_id.to_string()),
        public_history("bodySha256", body_sha256.into()),
        public_history("timeoutSeconds", timeout_seconds.to_string()),
    ];
    values.extend(
        parameters
            .iter()
            .map(|(name, parameter)| ExecutionParameter {
                name: format!("parameter.{name}"),
                display_value: display_parameter(parameter),
                sensitive: parameter.sensitive,
            }),
    );
    values
}

fn operation_parameter_summary(
    definition_id: Uuid,
    version_id: Uuid,
    body_sha256: &str,
    timeout_seconds: u64,
    parameters: &ValidatedParameters,
) -> Option<String> {
    let values = execution_history_parameters(
        definition_id,
        version_id,
        body_sha256,
        timeout_seconds,
        parameters,
    );
    let summary = values
        .iter()
        .map(|parameter| {
            let value = if parameter.sensitive {
                "[REDACTED]"
            } else {
                &parameter.display_value
            };
            format!("{}={value}", parameter.name)
        })
        .collect::<Vec<_>>()
        .join(", ");
    Some(cap_utf8(summary, 8 * 1024))
}

fn public_history(name: &str, display_value: String) -> ExecutionParameter {
    ExecutionParameter {
        name: name.into(),
        display_value,
        sensitive: false,
    }
}

fn display_parameter(parameter: &crate::core::tasks::ValidatedParameter) -> String {
    if parameter.sensitive {
        "[REDACTED]".into()
    } else if let Some(value) = parameter.value.as_str() {
        value.into()
    } else {
        parameter.value.to_string()
    }
}

fn cap_utf8(mut value: String, max_bytes: usize) -> String {
    if value.len() > max_bytes {
        let mut boundary = max_bytes;
        while !value.is_char_boundary(boundary) {
            boundary -= 1;
        }
        value.truncate(boundary);
    }
    value
}

fn operation_status(status: ExecutionStatus) -> OperationStatus {
    match status {
        ExecutionStatus::Succeeded => OperationStatus::Succeeded,
        ExecutionStatus::Cancelled => OperationStatus::Cancelled,
        ExecutionStatus::Uncertain => OperationStatus::Uncertain,
        ExecutionStatus::Failed | ExecutionStatus::Queued | ExecutionStatus::Running => {
            OperationStatus::Failed
        }
    }
}
