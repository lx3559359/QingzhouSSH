use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    core::{
        redaction::Redactor,
        ssh::{executor::EventSink, transport::execute_authenticated},
        system_probe::SystemCapabilities,
        tasks::{
            built_in_catalog, elevate_fixed_command, parse_result, plan_task, probe_privilege,
            select_implementation, task_version_is_compatible, validate_parameters, ExecutionScope,
            OperationConclusion, OperationResult, PlannedTask, PrivilegeMode, PrivilegeRequirement,
            RenderedTaskStep, RiskLevel, TaskCategory, TaskDefinition, ValidatedParameters,
        },
    },
    domain::{
        events::{ExecutionEvent, ExecutionEventPayload},
        execution::{now_millis, ExecutionParameter, ExecutionStatus},
        operation::{
            FinishOperationStep, NewOperationRun, NewOperationStep, OperationDetails,
            OperationFilter, OperationPhase, OperationRunRecord, OperationStatus,
            OperationStepStatus,
        },
        operation_restore::OperationRestorePointStatus,
    },
    error::{AppError, AppResult},
    repositories::operation_repository::OperationRepository,
    services::{
        execution_service::ExecutionService, operation_restore_service::OperationRestoreService,
        server_connector::ServerConnector,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationPreflightRequest {
    pub task_id: String,
    pub task_version: i32,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationStartRequest {
    pub task_id: String,
    pub task_version: i32,
    pub parameters: Value,
    pub confirmed_preview_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationPreview {
    pub preview_id: Uuid,
    pub server_id: String,
    pub task_id: String,
    pub task_version: i32,
    pub implementation_id: String,
    pub risk_level: RiskLevel,
    pub privilege: PrivilegeRequirement,
    pub scope: ExecutionScope,
    pub status: OperationStatus,
    pub step_titles: Vec<String>,
    pub estimated_seconds: u32,
}

#[derive(Clone)]
pub struct OperationService {
    repository: OperationRepository,
    executions: ExecutionService,
    restores: OperationRestoreService,
    connector: ServerConnector,
}

impl OperationService {
    pub fn new(
        repository: OperationRepository,
        executions: ExecutionService,
        restores: OperationRestoreService,
        connector: ServerConnector,
    ) -> Self {
        Self {
            repository,
            executions,
            restores,
            connector,
        }
    }

    pub async fn preflight(
        &self,
        server_id: &str,
        request: OperationPreflightRequest,
    ) -> AppResult<OperationPreview> {
        let (definition, run) = self.begin_preflight(server_id, &request).await?;
        let connected = match self.connector.connect(server_id).await {
            Ok(connected) => connected,
            Err(error) => {
                self.mark_preflight_failed(run.id).await;
                return Err(error);
            }
        };
        let privilege_mode = if definition.privilege == PrivilegeRequirement::RootOrPasswordlessSudo
        {
            match probe_privilege(&connected.session).await {
                Ok(mode) => Some(mode),
                Err(error) => {
                    self.mark_preflight_failed(run.id).await;
                    connected.session.disconnect().await;
                    return Err(error);
                }
            }
        } else {
            None
        };
        if definition.risk_level == RiskLevel::Dangerous {
            let plan = match plan_task(&definition, &connected.capabilities, &request.parameters) {
                Ok(plan) => plan,
                Err(error) => {
                    self.mark_preflight_failed(run.id).await;
                    connected.session.disconnect().await;
                    return Err(error);
                }
            };
            if let Err(error) = run_dangerous_preview(
                &connected.session,
                &plan.preview_steps,
                privilege_mode.unwrap_or(PrivilegeMode::Root),
            )
            .await
            {
                self.mark_preflight_failed(run.id).await;
                connected.session.disconnect().await;
                return Err(error);
            }
        }
        let result = self
            .complete_preflight(
                run,
                &definition,
                &request.parameters,
                &connected.capabilities,
            )
            .await;
        connected.session.disconnect().await;
        result
    }

    #[doc(hidden)]
    pub async fn preflight_with_capabilities(
        &self,
        server_id: &str,
        request: OperationPreflightRequest,
        capabilities: &SystemCapabilities,
    ) -> AppResult<OperationPreview> {
        let (definition, run) = self.begin_preflight(server_id, &request).await?;
        self.complete_preflight(run, &definition, &request.parameters, capabilities)
            .await
    }

    pub async fn start<E: EventSink>(
        &self,
        server_id: &str,
        request: OperationStartRequest,
        events: &mut E,
    ) -> AppResult<OperationDetails> {
        self.start_with_cancel(server_id, request, events, CancellationToken::new())
            .await
    }

    pub(crate) async fn start_with_cancel<E: EventSink>(
        &self,
        server_id: &str,
        request: OperationStartRequest,
        events: &mut E,
        cancel: CancellationToken,
    ) -> AppResult<OperationDetails> {
        let definition = resolve_definition(&request.task_id, request.task_version)?;
        validate_parameters(&definition, &request.parameters)?;
        require_dangerous_preview(&definition, request.confirmed_preview_id)?;
        let connected = tokio::select! {
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
            connected = self.connector.connect(server_id) => connected?,
        };
        let capabilities = connected.capabilities.clone();
        let privilege_mode = if definition.privilege == PrivilegeRequirement::RootOrPasswordlessSudo
        {
            match probe_privilege(&connected.session).await {
                Ok(mode) => Some(mode),
                Err(error) => {
                    connected.session.disconnect().await;
                    return Err(error);
                }
            }
        } else {
            None
        };
        connected.session.disconnect().await;
        self.start_with_capabilities_and_cancel(
            server_id,
            request,
            &capabilities,
            privilege_mode,
            events,
            &cancel,
        )
        .await
    }

    #[doc(hidden)]
    pub async fn start_with_capabilities<E: EventSink>(
        &self,
        server_id: &str,
        request: OperationStartRequest,
        capabilities: &SystemCapabilities,
        events: &mut E,
    ) -> AppResult<OperationDetails> {
        self.start_with_capabilities_and_cancel(
            server_id,
            request,
            capabilities,
            None,
            events,
            &CancellationToken::new(),
        )
        .await
    }

    async fn start_with_capabilities_and_cancel<E: EventSink>(
        &self,
        server_id: &str,
        request: OperationStartRequest,
        capabilities: &SystemCapabilities,
        privilege_mode: Option<PrivilegeMode>,
        events: &mut E,
        cancel: &CancellationToken,
    ) -> AppResult<OperationDetails> {
        let definition = resolve_definition(&request.task_id, request.task_version)?;
        let validated = validate_parameters(&definition, &request.parameters)?;
        require_dangerous_preview(&definition, request.confirmed_preview_id)?;
        let summary = parameter_summary(&validated);

        let preview_id = match request.confirmed_preview_id {
            Some(preview_id) => {
                self.require_matching_preview(preview_id, server_id, &definition, &summary)
                    .await?;
                preview_id
            }
            None => {
                self.preflight_with_capabilities(
                    server_id,
                    OperationPreflightRequest {
                        task_id: request.task_id.clone(),
                        task_version: request.task_version,
                        parameters: request.parameters.clone(),
                    },
                    capabilities,
                )
                .await?
                .preview_id
            }
        };

        let implementation = select_implementation(&definition, capabilities)?;
        let plan = plan_task(&definition, capabilities, &request.parameters)?;
        if definition.risk_level == RiskLevel::Dangerous {
            return self
                .run_dangerous(
                    preview_id,
                    server_id,
                    &definition,
                    &plan,
                    privilege_mode.unwrap_or(PrivilegeMode::Root),
                    events,
                    cancel,
                )
                .await;
        }

        if implementation.backup_plan.is_some() {
            return Err(AppError::Validation(
                "该任务需要备份与恢复流程，当前阶段不能直接运行".into(),
            ));
        }
        if cancel.is_cancelled() {
            self.repository
                .transition(preview_id, OperationStatus::Cancelled)
                .await?;
            return self.require_details(preview_id).await;
        }
        self.repository
            .transition(preview_id, OperationStatus::Running)
            .await?;
        let history_parameters = history_parameters(&plan.parameters);
        let mut operation_output = String::new();

        for (index, step) in plan.execution_steps.iter().enumerate() {
            if cancel.is_cancelled() {
                self.repository
                    .skip_pending_steps(preview_id, OperationPhase::Execute, index)
                    .await?;
                self.repository
                    .transition(preview_id, OperationStatus::Cancelled)
                    .await?;
                return self.require_details(preview_id).await;
            }
            self.repository
                .mark_step_running(preview_id, OperationPhase::Execute, index, now_millis())
                .await?;
            let execution = {
                let mut capture = OperationOutputSink::new(events, &mut operation_output);
                self.executions
                    .execute_planned_step_with_cancel(
                        server_id,
                        &definition.id,
                        definition.version,
                        execution_history_category(definition.category),
                        step,
                        &history_parameters,
                        &mut capture,
                        cancel,
                    )
                    .await
            };
            let details = match execution {
                Ok(details) => details,
                Err(error) => {
                    self.repository
                        .finish_step(FinishOperationStep {
                            run_id: preview_id,
                            phase: OperationPhase::Execute,
                            step_index: index,
                            status: OperationStepStatus::Failed,
                            execution_id: None,
                            output_summary: None,
                            error_message: Some(error.to_string()),
                            finished_at: now_millis(),
                        })
                        .await?;
                    self.repository
                        .skip_pending_steps(
                            preview_id,
                            OperationPhase::Execute,
                            index.saturating_add(1),
                        )
                        .await?;
                    self.repository
                        .transition(preview_id, OperationStatus::Failed)
                        .await?;
                    return Err(error);
                }
            };
            let (step_status, run_status) = statuses_for_execution(details.record.status);
            self.repository
                .finish_step(FinishOperationStep {
                    run_id: preview_id,
                    phase: OperationPhase::Execute,
                    step_index: index,
                    status: step_status,
                    execution_id: Some(details.record.id),
                    output_summary: details.record.output_summary.clone(),
                    error_message: details.record.error_message.clone(),
                    finished_at: details.record.finished_at.unwrap_or_else(now_millis),
                })
                .await?;
            if let Some(run_status) = run_status {
                self.repository
                    .skip_pending_steps(
                        preview_id,
                        OperationPhase::Execute,
                        index.saturating_add(1),
                    )
                    .await?;
                self.repository.transition(preview_id, run_status).await?;
                return self.require_details(preview_id).await;
            }
        }

        let result = match parse_result(plan.result_parser, &operation_output, &Redactor::default())
        {
            Ok(result) => result,
            Err(error) => parser_warning_result("远程任务已执行成功", error),
        };
        if let Err(error) = self.repository.set_result(preview_id, &result).await {
            let _ = self
                .repository
                .transition(preview_id, OperationStatus::Failed)
                .await;
            return Err(error);
        }

        self.repository
            .transition(preview_id, OperationStatus::Succeeded)
            .await?;
        self.require_details(preview_id).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_dangerous<E: EventSink>(
        &self,
        run_id: Uuid,
        server_id: &str,
        definition: &TaskDefinition,
        plan: &PlannedTask,
        privilege_mode: PrivilegeMode,
        events: &mut E,
        cancel: &CancellationToken,
    ) -> AppResult<OperationDetails> {
        if !matches!(
            definition.id.as_str(),
            "system.hostname_change"
                | "system.timezone_change"
                | "storage.swap_manage"
                | "security.file_permissions"
        ) {
            return Err(AppError::Validation(
                "该危险任务的恢复执行器尚未完成，服务器未发生修改".into(),
            ));
        }
        let backup_plan = plan
            .backup_plan
            .as_ref()
            .ok_or_else(|| AppError::Validation("危险任务缺少恢复点声明".into()))?;
        if plan.verify_steps.is_empty() || plan.rollback_plan.is_none() {
            return Err(AppError::Validation(
                "危险任务缺少验证或回滚声明，已阻止运行".into(),
            ));
        }
        self.repository
            .transition(run_id, OperationStatus::WaitingConfirmation)
            .await?;
        if cancel.is_cancelled() {
            self.repository
                .transition(run_id, OperationStatus::Cancelled)
                .await?;
            return self.require_details(run_id).await;
        }
        self.repository
            .transition(run_id, OperationStatus::BackingUp)
            .await?;
        self.repository
            .mark_step_running(run_id, OperationPhase::Backup, 0, now_millis())
            .await?;
        let restore = match self
            .restores
            .capture(
                run_id,
                server_id,
                &definition.id,
                &plan.implementation_id,
                backup_plan,
                &plan.parameters,
                cancel.child_token(),
            )
            .await
        {
            Ok(restore) => restore,
            Err(error) => {
                self.repository
                    .finish_step(FinishOperationStep {
                        run_id,
                        phase: OperationPhase::Backup,
                        step_index: 0,
                        status: OperationStepStatus::Failed,
                        execution_id: None,
                        output_summary: None,
                        error_message: Some(error.to_string()),
                        finished_at: now_millis(),
                    })
                    .await?;
                self.repository
                    .skip_pending_steps(run_id, OperationPhase::Backup, 1)
                    .await?;
                self.repository
                    .transition(
                        run_id,
                        if matches!(error, AppError::Cancelled) {
                            OperationStatus::Cancelled
                        } else {
                            OperationStatus::Failed
                        },
                    )
                    .await?;
                return self.require_details(run_id).await;
            }
        };
        for index in 0..backup_plan.items.len() {
            if index > 0 {
                self.repository
                    .mark_step_running(run_id, OperationPhase::Backup, index, now_millis())
                    .await?;
            }
            self.repository
                .finish_step(FinishOperationStep {
                    run_id,
                    phase: OperationPhase::Backup,
                    step_index: index,
                    status: OperationStepStatus::Succeeded,
                    execution_id: None,
                    output_summary: Some("恢复项已校验并保存在项目数据目录".into()),
                    error_message: None,
                    finished_at: now_millis(),
                })
                .await?;
        }

        let history_parameters = history_parameters(&plan.parameters);
        let execution_steps = elevate_steps(&plan.execution_steps, privilege_mode)?;
        let verify_steps = elevate_steps(&plan.verify_steps, privilege_mode)?;
        let mut operation_output = String::new();

        self.repository
            .transition(run_id, OperationStatus::Running)
            .await?;
        if let Some(status) = self
            .run_phase(
                run_id,
                server_id,
                definition,
                OperationPhase::Execute,
                &execution_steps,
                &history_parameters,
                events,
                cancel,
                &mut operation_output,
            )
            .await?
        {
            return self
                .finish_dangerous_failure(run_id, restore.point.id, status)
                .await;
        }

        self.repository
            .transition(run_id, OperationStatus::Verifying)
            .await?;
        if let Some(status) = self
            .run_phase(
                run_id,
                server_id,
                definition,
                OperationPhase::Verify,
                &verify_steps,
                &history_parameters,
                events,
                cancel,
                &mut operation_output,
            )
            .await?
        {
            return self
                .finish_dangerous_failure(run_id, restore.point.id, status)
                .await;
        }

        let result = match parse_result(plan.result_parser, &operation_output, &Redactor::default())
        {
            Ok(result) => result,
            Err(error) => parser_warning_result("远程目标状态已验证成功", error),
        };
        self.repository.set_result(run_id, &result).await?;
        self.repository
            .transition(run_id, OperationStatus::Succeeded)
            .await?;
        self.require_details(run_id).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_phase<E: EventSink>(
        &self,
        run_id: Uuid,
        server_id: &str,
        definition: &TaskDefinition,
        phase: OperationPhase,
        steps: &[RenderedTaskStep],
        history_parameters: &[ExecutionParameter],
        events: &mut E,
        cancel: &CancellationToken,
        operation_output: &mut String,
    ) -> AppResult<Option<OperationStatus>> {
        for (index, step) in steps.iter().enumerate() {
            if cancel.is_cancelled() {
                self.repository
                    .skip_pending_steps(run_id, phase, index)
                    .await?;
                return Ok(Some(OperationStatus::Cancelled));
            }
            self.repository
                .mark_step_running(run_id, phase, index, now_millis())
                .await?;
            let execution = {
                let mut capture = OperationOutputSink::new(events, operation_output);
                self.executions
                    .execute_planned_step_with_cancel(
                        server_id,
                        &definition.id,
                        definition.version,
                        execution_history_category(definition.category),
                        step,
                        history_parameters,
                        &mut capture,
                        cancel,
                    )
                    .await
            };
            let details = match execution {
                Ok(details) => details,
                Err(error) => {
                    self.repository
                        .finish_step(FinishOperationStep {
                            run_id,
                            phase,
                            step_index: index,
                            status: OperationStepStatus::Failed,
                            execution_id: None,
                            output_summary: None,
                            error_message: Some(error.to_string()),
                            finished_at: now_millis(),
                        })
                        .await?;
                    self.repository
                        .skip_pending_steps(run_id, phase, index.saturating_add(1))
                        .await?;
                    return Ok(Some(operation_status_for_error(&error)));
                }
            };
            let (step_status, mut run_status) = statuses_for_execution(details.record.status);
            if run_status == Some(OperationStatus::Failed)
                && details
                    .record
                    .error_category
                    .as_deref()
                    .is_some_and(|category| {
                        matches!(
                            category,
                            "ssh" | "io" | "transfer" | "remote_state_uncertain"
                        )
                    })
            {
                run_status = Some(OperationStatus::Uncertain);
            }
            self.repository
                .finish_step(FinishOperationStep {
                    run_id,
                    phase,
                    step_index: index,
                    status: step_status,
                    execution_id: Some(details.record.id),
                    output_summary: details.record.output_summary.clone(),
                    error_message: details.record.error_message.clone(),
                    finished_at: details.record.finished_at.unwrap_or_else(now_millis),
                })
                .await?;
            if let Some(status) = run_status {
                self.repository
                    .skip_pending_steps(run_id, phase, index.saturating_add(1))
                    .await?;
                return Ok(Some(status));
            }
        }
        Ok(None)
    }

    async fn finish_dangerous_failure(
        &self,
        run_id: Uuid,
        restore_point_id: Uuid,
        failure_status: OperationStatus,
    ) -> AppResult<OperationDetails> {
        if failure_status == OperationStatus::Uncertain {
            self.repository
                .transition(run_id, OperationStatus::Uncertain)
                .await?;
            return self.require_details(run_id).await;
        }
        self.repository
            .transition(run_id, OperationStatus::RollbackAvailable)
            .await?;
        self.repository
            .transition(run_id, OperationStatus::RollingBack)
            .await?;
        self.repository
            .mark_step_running(run_id, OperationPhase::Rollback, 0, now_millis())
            .await?;
        let rollback = self
            .restores
            .rollback(restore_point_id, CancellationToken::new())
            .await;
        let (step_status, run_status, message) = match rollback {
            Ok(details) => match details.point.status {
                OperationRestorePointStatus::RolledBack => (
                    OperationStepStatus::Succeeded,
                    OperationStatus::RolledBack,
                    None,
                ),
                OperationRestorePointStatus::Partial => (
                    OperationStepStatus::Failed,
                    OperationStatus::RollbackPartial,
                    Some("只恢复了部分项目，请查看恢复点详情".into()),
                ),
                _ => (
                    OperationStepStatus::Failed,
                    OperationStatus::RollbackFailed,
                    Some("自动回滚失败，恢复点仍保留".into()),
                ),
            },
            Err(error) => {
                let uncertain = operation_status_for_error(&error) == OperationStatus::Uncertain;
                (
                    if uncertain {
                        OperationStepStatus::Uncertain
                    } else {
                        OperationStepStatus::Failed
                    },
                    if uncertain {
                        OperationStatus::Uncertain
                    } else {
                        OperationStatus::RollbackFailed
                    },
                    Some(error.to_string()),
                )
            }
        };
        self.repository
            .finish_step(FinishOperationStep {
                run_id,
                phase: OperationPhase::Rollback,
                step_index: 0,
                status: step_status,
                execution_id: None,
                output_summary: (step_status == OperationStepStatus::Succeeded)
                    .then(|| "已核验恢复到修改前状态".into()),
                error_message: message,
                finished_at: now_millis(),
            })
            .await?;
        self.repository.transition(run_id, run_status).await?;
        self.require_details(run_id).await
    }

    pub async fn get(&self, id: Uuid) -> AppResult<Option<OperationDetails>> {
        self.repository.get(id).await
    }

    pub async fn list(&self, filter: OperationFilter) -> AppResult<Vec<OperationRunRecord>> {
        self.repository.list(filter).await
    }

    pub async fn cancel(&self, id: Uuid) -> AppResult<()> {
        let details = self
            .repository
            .get(id)
            .await?
            .ok_or_else(|| AppError::Validation("运维运行不存在".into()))?;
        if matches!(
            details.run.status,
            OperationStatus::BackingUp
                | OperationStatus::Running
                | OperationStatus::Verifying
                | OperationStatus::RollingBack
        ) {
            return Err(AppError::Validation(
                "任务正在远程执行，请在当前步骤完成取消接入后再取消".into(),
            ));
        }
        self.repository
            .transition(id, OperationStatus::Cancelled)
            .await
    }

    async fn begin_preflight(
        &self,
        server_id: &str,
        request: &OperationPreflightRequest,
    ) -> AppResult<(TaskDefinition, OperationRunRecord)> {
        let definition = resolve_definition(&request.task_id, request.task_version)?;
        let validated = validate_parameters(&definition, &request.parameters)?;
        self.connector.require_server(server_id).await?;
        let run = self
            .repository
            .create(NewOperationRun {
                server_id: server_id.into(),
                task_id: definition.id.clone(),
                task_version: definition.version,
                risk_level: definition.risk_level,
                parameters_summary: parameter_summary(&validated),
            })
            .await?;
        self.repository
            .transition(run.id, OperationStatus::Preflighting)
            .await?;
        Ok((definition, run))
    }

    async fn complete_preflight(
        &self,
        run: OperationRunRecord,
        definition: &TaskDefinition,
        parameters: &Value,
        capabilities: &SystemCapabilities,
    ) -> AppResult<OperationPreview> {
        let plan = match plan_task(definition, capabilities, parameters) {
            Ok(plan) => plan,
            Err(error) => {
                self.mark_preflight_failed(run.id).await;
                return Err(error);
            }
        };
        if let Err(error) = self.create_steps(run.id, &plan).await {
            self.mark_preflight_failed(run.id).await;
            return Err(error);
        }
        self.repository
            .transition(run.id, OperationStatus::PreviewReady)
            .await?;
        let summary = plan.public_summary();
        Ok(OperationPreview {
            preview_id: run.id,
            server_id: run.server_id,
            task_id: summary.definition_id,
            task_version: summary.definition_version,
            implementation_id: summary.implementation_id,
            risk_level: summary.risk_level,
            privilege: summary.privilege,
            scope: definition.scope,
            status: OperationStatus::PreviewReady,
            step_titles: summary.step_titles,
            estimated_seconds: summary.estimated_seconds,
        })
    }

    async fn create_steps(&self, run_id: Uuid, plan: &PlannedTask) -> AppResult<()> {
        for (index, step) in plan.preflight_steps.iter().enumerate() {
            self.repository
                .create_step(NewOperationStep {
                    run_id,
                    phase: OperationPhase::Preflight,
                    step_index: index,
                    step_id: step.id.clone(),
                    title: step.title.clone(),
                })
                .await?;
        }
        if let Some(backup_plan) = &plan.backup_plan {
            for (index, item) in backup_plan.items.iter().enumerate() {
                self.repository
                    .create_step(NewOperationStep {
                        run_id,
                        phase: OperationPhase::Backup,
                        step_index: index,
                        step_id: item.id.clone(),
                        title: format!("创建恢复项：{}", item.id),
                    })
                    .await?;
            }
        }
        for (index, step) in plan.execution_steps.iter().enumerate() {
            self.repository
                .create_step(NewOperationStep {
                    run_id,
                    phase: OperationPhase::Execute,
                    step_index: index,
                    step_id: step.id.clone(),
                    title: step.title.clone(),
                })
                .await?;
        }
        for (index, step) in plan.verify_steps.iter().enumerate() {
            self.repository
                .create_step(NewOperationStep {
                    run_id,
                    phase: OperationPhase::Verify,
                    step_index: index,
                    step_id: step.id.clone(),
                    title: step.title.clone(),
                })
                .await?;
        }
        if let Some(rollback_plan) = &plan.rollback_plan {
            for (index, step) in rollback_plan.steps.iter().enumerate() {
                self.repository
                    .create_step(NewOperationStep {
                        run_id,
                        phase: OperationPhase::Rollback,
                        step_index: index,
                        step_id: step.id.clone(),
                        title: step.title.clone(),
                    })
                    .await?;
            }
        }
        Ok(())
    }

    async fn require_matching_preview(
        &self,
        preview_id: Uuid,
        server_id: &str,
        definition: &TaskDefinition,
        parameters_summary: &Option<String>,
    ) -> AppResult<()> {
        let details = self
            .repository
            .get(preview_id)
            .await?
            .ok_or_else(|| AppError::Validation("确认的预览不存在".into()))?;
        if details.run.server_id != server_id
            || details.run.task_id != definition.id
            || details.run.task_version != definition.version
            || details.run.status != OperationStatus::PreviewReady
            || details.run.parameters_summary != *parameters_summary
        {
            return Err(AppError::Validation(
                "确认的预览与本次任务或参数不一致".into(),
            ));
        }
        Ok(())
    }

    async fn require_details(&self, id: Uuid) -> AppResult<OperationDetails> {
        self.repository
            .get(id)
            .await?
            .ok_or_else(|| AppError::Validation("运维运行不存在".into()))
    }

    async fn mark_preflight_failed(&self, id: Uuid) {
        let _ = self
            .repository
            .transition(id, OperationStatus::Failed)
            .await;
    }
}

const MAX_CAPTURED_OPERATION_OUTPUT: usize = 1024 * 1024;

struct OperationOutputSink<'a, E: EventSink> {
    inner: &'a mut E,
    output: &'a mut String,
}

impl<'a, E: EventSink> OperationOutputSink<'a, E> {
    fn new(inner: &'a mut E, output: &'a mut String) -> Self {
        Self { inner, output }
    }

    fn append(&mut self, text: &str) {
        if self.output.len() >= MAX_CAPTURED_OPERATION_OUTPUT {
            return;
        }
        let remaining = MAX_CAPTURED_OPERATION_OUTPUT - self.output.len();
        if text.len() <= remaining {
            self.output.push_str(text);
            return;
        }
        let mut boundary = remaining;
        while !text.is_char_boundary(boundary) {
            boundary -= 1;
        }
        self.output.push_str(&text[..boundary]);
    }
}

impl<E: EventSink> EventSink for OperationOutputSink<'_, E> {
    fn send(&mut self, event: ExecutionEvent) -> AppResult<()> {
        match &event.payload {
            ExecutionEventPayload::Stdout { text, .. }
            | ExecutionEventPayload::Stderr { text, .. } => self.append(text),
            _ => {}
        }
        self.inner.send(event)
    }
}

fn resolve_definition(task_id: &str, requested_version: i32) -> AppResult<TaskDefinition> {
    let definition = built_in_catalog()
        .into_iter()
        .find(|definition| definition.id == task_id)
        .ok_or_else(|| AppError::Validation("运维任务不存在".into()))?;
    if !task_version_is_compatible(&definition, requested_version) {
        return Err(AppError::Validation("运维任务版本不存在".into()));
    }
    Ok(definition)
}

fn require_dangerous_preview(
    definition: &TaskDefinition,
    preview_id: Option<Uuid>,
) -> AppResult<()> {
    if definition.risk_level == RiskLevel::Dangerous && preview_id.is_none() {
        return Err(AppError::Validation(
            "危险任务必须先预览并确认影响范围".into(),
        ));
    }
    Ok(())
}

fn statuses_for_execution(
    status: ExecutionStatus,
) -> (OperationStepStatus, Option<OperationStatus>) {
    match status {
        ExecutionStatus::Succeeded => (OperationStepStatus::Succeeded, None),
        ExecutionStatus::Cancelled => (
            OperationStepStatus::Cancelled,
            Some(OperationStatus::Cancelled),
        ),
        ExecutionStatus::Uncertain => (
            OperationStepStatus::Uncertain,
            Some(OperationStatus::Uncertain),
        ),
        ExecutionStatus::Queued | ExecutionStatus::Running | ExecutionStatus::Failed => {
            (OperationStepStatus::Failed, Some(OperationStatus::Failed))
        }
    }
}

fn operation_status_for_error(error: &AppError) -> OperationStatus {
    match error {
        AppError::Cancelled => OperationStatus::Cancelled,
        AppError::RemoteStateUncertain(_)
        | AppError::Ssh(_)
        | AppError::Io(_)
        | AppError::Transfer(_) => OperationStatus::Uncertain,
        _ => OperationStatus::Failed,
    }
}

fn parser_warning_result(summary: &str, error: AppError) -> OperationResult {
    OperationResult {
        status: OperationConclusion::Warning,
        summary: summary.into(),
        findings: Vec::new(),
        suggestions: vec!["结果已完成，但结构化展示失败；可查看执行记录。".into()],
        technical_details: error.to_string(),
    }
}

fn elevate_steps(
    steps: &[RenderedTaskStep],
    privilege_mode: PrivilegeMode,
) -> AppResult<Vec<RenderedTaskStep>> {
    steps
        .iter()
        .map(|step| {
            let mut elevated = step.clone();
            elevated.command = elevate_fixed_command(&step.command, privilege_mode)?;
            Ok(elevated)
        })
        .collect()
}

async fn run_dangerous_preview(
    session: &crate::core::ssh::transport::AuthenticatedSshSession,
    steps: &[RenderedTaskStep],
    privilege_mode: PrivilegeMode,
) -> AppResult<()> {
    for step in steps {
        let command = elevate_fixed_command(&step.command, privilege_mode)?;
        let output = tokio::time::timeout(
            Duration::from_secs(step.timeout_seconds),
            execute_authenticated(session, &command),
        )
        .await
        .map_err(|_| AppError::RemoteStateUncertain("危险任务只读预演超时".into()))??;
        if output.exit_status != 0 {
            return Err(AppError::ssh_command(output.exit_status, output.stderr));
        }
    }
    Ok(())
}

fn execution_history_category(category: TaskCategory) -> &'static str {
    match category {
        TaskCategory::System => "system",
        TaskCategory::Service => "service",
        TaskCategory::Logs => "logs",
        TaskCategory::Storage
        | TaskCategory::Network
        | TaskCategory::Security
        | TaskCategory::Web
        | TaskCategory::Container
        | TaskCategory::Script
        | TaskCategory::Advanced => "advanced",
    }
}

fn parameter_summary(parameters: &ValidatedParameters) -> Option<String> {
    let summary = parameters
        .iter()
        .map(|(name, parameter)| {
            let value = if parameter.sensitive {
                "[REDACTED]".into()
            } else if let Some(value) = parameter.value.as_str() {
                value.into()
            } else {
                parameter.value.to_string()
            };
            format!("{name}={value}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    (!summary.is_empty()).then_some(summary)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ssh::executor::VecEventSink;

    #[test]
    fn operation_output_sink_forwards_events_and_caps_utf8_capture() {
        let mut forwarded = VecEventSink::default();
        let mut output = String::new();
        {
            let mut sink = OperationOutputSink::new(&mut forwarded, &mut output);
            sink.send(ExecutionEvent {
                sequence: 1,
                emitted_at: 1,
                payload: ExecutionEventPayload::Stdout {
                    text: "正常\n".into(),
                    total_bytes: 7,
                },
            })
            .unwrap();
            sink.send(ExecutionEvent {
                sequence: 2,
                emitted_at: 2,
                payload: ExecutionEventPayload::Stderr {
                    text: "错".repeat(MAX_CAPTURED_OPERATION_OUTPUT),
                    total_bytes: u64::try_from(MAX_CAPTURED_OPERATION_OUTPUT).unwrap(),
                },
            })
            .unwrap();
        }
        assert_eq!(forwarded.events.len(), 2);
        assert!(output.starts_with("正常\n"));
        assert!(output.len() <= MAX_CAPTURED_OPERATION_OUTPUT);
        assert!(output.is_char_boundary(output.len()));
    }
}
