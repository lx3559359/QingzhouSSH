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
        operation_restore::{OperationRestorePoint, OperationRestorePointStatus},
        server::ServerProfile,
    },
    error::{AppError, AppResult},
    repositories::operation_repository::OperationRepository,
    services::{
        execution_service::ExecutionService,
        operation_restore_service::OperationRestoreService,
        remote_recovery_service::{
            build_ip_change_recovery_plan, parse_ip_recovery_state, IpRecoveryState,
            RemoteRecoveryService,
        },
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationConfirmRequest {
    pub task_id: String,
    pub task_version: i32,
    pub parameters: Value,
    pub confirmation_token: Uuid,
}

impl From<OperationConfirmRequest> for OperationStartRequest {
    fn from(value: OperationConfirmRequest) -> Self {
        Self {
            task_id: value.task_id,
            task_version: value.task_version,
            parameters: value.parameters,
            confirmed_preview_id: Some(value.confirmation_token),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationPreviewServer {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
}

impl From<ServerProfile> for OperationPreviewServer {
    fn from(value: ServerProfile) -> Self {
        Self {
            id: value.id,
            name: value.name,
            host: value.host,
            port: value.port,
            username: value.username,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationDisconnectRisk {
    pub may_disconnect: bool,
    pub explanation: Option<String>,
    pub automatic_recovery_seconds: Option<u64>,
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
    pub confirmation_token: Option<Uuid>,
    pub server: OperationPreviewServer,
    pub permission_summary: String,
    pub current_state_summary: String,
    pub target_state_summary: String,
    pub backup_summary: Vec<String>,
    pub disconnect_risk: OperationDisconnectRisk,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRecoveryResult {
    pub operation: OperationDetails,
    pub what_happened: String,
    pub server_may_have_changed: bool,
    pub state_confirmed: bool,
    pub next_step: String,
    pub restore_point: Option<OperationRestorePoint>,
    pub technical_details: Option<String>,
}

struct OperationPreviewContext {
    server: ServerProfile,
    permission_summary: String,
    current_state_summary: String,
}

#[derive(Clone)]
pub struct OperationService {
    repository: OperationRepository,
    executions: ExecutionService,
    restores: OperationRestoreService,
    remote_recovery: RemoteRecoveryService,
    connector: ServerConnector,
}

impl OperationService {
    pub fn new(
        repository: OperationRepository,
        executions: ExecutionService,
        restores: OperationRestoreService,
        remote_recovery: RemoteRecoveryService,
        connector: ServerConnector,
    ) -> Self {
        Self {
            repository,
            executions,
            restores,
            remote_recovery,
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
        let current_state_summary = if definition.risk_level == RiskLevel::Dangerous {
            let plan = match plan_task(&definition, &connected.capabilities, &request.parameters) {
                Ok(plan) => plan,
                Err(error) => {
                    self.mark_preflight_failed(run.id).await;
                    connected.session.disconnect().await;
                    return Err(error);
                }
            };
            match run_dangerous_preview(
                &connected.session,
                &plan.preview_steps,
                privilege_mode.unwrap_or(PrivilegeMode::Root),
                &connected.redactor,
            )
            .await
            {
                Ok(summary) => summary,
                Err(error) => {
                    self.mark_preflight_failed(run.id).await;
                    connected.session.disconnect().await;
                    return Err(error);
                }
            }
        } else {
            "只读任务将在执行时读取服务器当前状态".into()
        };
        let server = connected.profile.clone();
        let permission_summary = permission_summary(definition.privilege, privilege_mode);
        let result = self
            .complete_preflight(
                run,
                &definition,
                &request.parameters,
                &connected.capabilities,
                OperationPreviewContext {
                    server,
                    permission_summary,
                    current_state_summary,
                },
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
        let server = self.connector.require_server(server_id).await?;
        let permission = permission_summary(definition.privilege, None);
        self.complete_preflight(
            run,
            &definition,
            &request.parameters,
            capabilities,
            OperationPreviewContext {
                server,
                permission_summary: permission,
                current_state_summary: "测试能力预检未读取远程当前状态".into(),
            },
        )
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
                | "service.start"
                | "service.stop"
                | "service.restart"
                | "service.boot_policy"
                | "service.cron_manage"
                | "container.action"
                | "network.hosts_manage"
                | "security.firewall_open_port"
                | "network.ip_change"
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
        let ip_recovery = if definition.id == "network.ip_change" {
            let recovery = build_ip_change_recovery_plan(
                run_id,
                &plan.implementation_id,
                required_parameter_str(&plan.parameters, "interface")?,
                required_parameter_str(&plan.parameters, "cidr")?,
                required_parameter_str(&plan.parameters, "gateway")?,
                required_parameter_u64(&plan.parameters, "rollbackSeconds")?,
            )?;
            self.restores
                .attach_remote_asset(
                    restore.point.id,
                    &format!("qingzhou-recovery/{run_id}"),
                    now_millis() + 24 * 60 * 60 * 1_000,
                )
                .await?;
            Some(recovery)
        } else {
            None
        };
        let network_steps;
        let execution_steps = if let Some(recovery) = &ip_recovery {
            network_steps = vec![
                RenderedTaskStep {
                    id: "arm-rollback".into(),
                    title: "安排超时自动恢复".into(),
                    command: elevate_fixed_command(&recovery.arm_command, privilege_mode)?,
                    timeout_seconds: 45,
                    output_limit_bytes: 64 * 1024,
                },
                RenderedTaskStep {
                    id: "apply-network".into(),
                    title: "暂存新网络地址".into(),
                    command: elevate_fixed_command(&recovery.apply_command, privilege_mode)?,
                    timeout_seconds: 45,
                    output_limit_bytes: 64 * 1024,
                },
            ];
            network_steps.as_slice()
        } else {
            network_steps = elevate_steps(&plan.execution_steps, privilege_mode)?;
            network_steps.as_slice()
        };
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
                execution_steps,
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
        if let Some(recovery) = &ip_recovery {
            self.repository
                .mark_step_running(run_id, OperationPhase::Verify, 0, now_millis())
                .await?;
            if cancel.is_cancelled() {
                self.repository
                    .finish_step(FinishOperationStep {
                        run_id,
                        phase: OperationPhase::Verify,
                        step_index: 0,
                        status: OperationStepStatus::Uncertain,
                        execution_id: None,
                        output_summary: None,
                        error_message: Some(
                            "已安排远端自动恢复；取消后等待服务器恢复原网络".into(),
                        ),
                        finished_at: now_millis(),
                    })
                    .await?;
                return self
                    .finish_dangerous_failure(run_id, restore.point.id, OperationStatus::Uncertain)
                    .await;
            }
            let finalized = self
                .remote_recovery
                .finalize_ip_change(
                    server_id,
                    &recovery.target_host,
                    &recovery.finalize_command,
                    privilege_mode,
                )
                .await;
            match finalized {
                Ok(output) => {
                    operation_output.push_str(&output.stdout);
                    if let Err(error) = self
                        .connector
                        .commit_verified_host_change(server_id, &recovery.target_host)
                        .await
                    {
                        self.repository
                            .finish_step(FinishOperationStep {
                                run_id,
                                phase: OperationPhase::Verify,
                                step_index: 0,
                                status: OperationStepStatus::Uncertain,
                                execution_id: None,
                                output_summary: Some(
                                    "新 IP 已验证，但本地服务器地址更新失败".into(),
                                ),
                                error_message: Some(error.to_string()),
                                finished_at: now_millis(),
                            })
                            .await?;
                        self.repository
                            .transition(run_id, OperationStatus::Uncertain)
                            .await?;
                        return self.require_details(run_id).await;
                    }
                    self.repository
                        .finish_step(FinishOperationStep {
                            run_id,
                            phase: OperationPhase::Verify,
                            step_index: 0,
                            status: OperationStepStatus::Succeeded,
                            execution_id: None,
                            output_summary: Some(
                                "已通过新 IP 建立独立 SSH 连接并核验地址、路由和主机指纹".into(),
                            ),
                            error_message: None,
                            finished_at: now_millis(),
                        })
                        .await?;
                }
                Err(error) => {
                    self.repository
                        .finish_step(FinishOperationStep {
                            run_id,
                            phase: OperationPhase::Verify,
                            step_index: 0,
                            status: OperationStepStatus::Uncertain,
                            execution_id: None,
                            output_summary: Some(
                                "未能通过新 IP 完成独立验证；远端定时恢复保持有效".into(),
                            ),
                            error_message: Some(error.to_string()),
                            finished_at: now_millis(),
                        })
                        .await?;
                    return self
                        .finish_dangerous_failure(
                            run_id,
                            restore.point.id,
                            OperationStatus::Uncertain,
                        )
                        .await;
                }
            }
        } else if let Some(status) = self
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

    pub async fn reconcile_ip_change(&self, run_id: Uuid) -> AppResult<OperationDetails> {
        let details = self
            .repository
            .get(run_id)
            .await?
            .ok_or_else(|| AppError::Validation("运维运行不存在".into()))?;
        if details.run.task_id != "network.ip_change"
            || details.run.status != OperationStatus::Uncertain
        {
            return Err(AppError::Validation(
                "只有状态未确认的 IP 修改可以重新连接核验".into(),
            ));
        }
        let parameters = parse_network_parameter_summary(
            details
                .run
                .parameters_summary
                .as_deref()
                .ok_or_else(|| AppError::Integrity("IP 修改运行缺少参数摘要".into()))?,
        )?;
        let target = parameters
            .get("cidr")
            .and_then(Value::as_str)
            .and_then(|cidr| cidr.split_once('/').map(|(address, _)| address))
            .ok_or_else(|| AppError::Integrity("IP 修改运行的新地址无效".into()))?;

        let current = self.connector.connect(&details.run.server_id).await;
        let (capabilities, privilege_mode, inspect_current) = match current {
            Ok(connected) => {
                let capabilities = connected.capabilities.clone();
                let privilege_mode = probe_privilege(&connected.session).await?;
                connected.session.disconnect().await;
                (capabilities, privilege_mode, true)
            }
            Err(_) => {
                let connected = self
                    .connector
                    .connect_at_verified_ip(&details.run.server_id, target)
                    .await?;
                let capabilities = connected.capabilities.clone();
                let privilege_mode = probe_privilege(&connected.session).await?;
                connected.session.disconnect().await;
                (capabilities, privilege_mode, false)
            }
        };
        let definition = resolve_definition("network.ip_change", details.run.task_version)?;
        let planned = plan_task(&definition, &capabilities, &parameters)?;
        let validated = validate_parameters(&definition, &parameters)?;
        let recovery = build_ip_change_recovery_plan(
            run_id,
            &planned.implementation_id,
            required_parameter_str(&validated, "interface")?,
            required_parameter_str(&validated, "cidr")?,
            required_parameter_str(&validated, "gateway")?,
            required_parameter_u64(&validated, "rollbackSeconds")?,
        )?;
        let observation = if inspect_current {
            self.remote_recovery
                .inspect_ip_change_current(
                    &details.run.server_id,
                    &recovery.inspect_command,
                    privilege_mode,
                )
                .await?
        } else {
            self.remote_recovery
                .inspect_ip_change(
                    &details.run.server_id,
                    &recovery.target_host,
                    &recovery.inspect_command,
                    privilege_mode,
                )
                .await?
        };
        let mut state =
            parse_ip_recovery_state(&observation.stdout, &recovery.rollback_script_sha256)?;
        if state == IpRecoveryState::Staged
            && self
                .remote_recovery
                .finalize_ip_change(
                    &details.run.server_id,
                    &recovery.target_host,
                    &recovery.finalize_command,
                    privilege_mode,
                )
                .await
                .is_ok()
        {
            state = IpRecoveryState::Committed;
        }
        match state {
            IpRecoveryState::Committed => {
                self.connector
                    .commit_verified_host_change(&details.run.server_id, &recovery.target_host)
                    .await?;
                let result = OperationResult {
                    status: OperationConclusion::Normal,
                    summary: "已重新连接新 IP，并确认地址、路由和主机指纹".into(),
                    findings: Vec::new(),
                    suggestions: Vec::new(),
                    technical_details: "reconciled=committed".into(),
                };
                self.repository.set_result(run_id, &result).await?;
                if details.steps.iter().any(|step| {
                    step.phase == OperationPhase::Verify
                        && step.step_index == 0
                        && step.status == OperationStepStatus::Uncertain
                }) {
                    self.repository
                        .finish_step(FinishOperationStep {
                            run_id,
                            phase: OperationPhase::Verify,
                            step_index: 0,
                            status: OperationStepStatus::Succeeded,
                            execution_id: None,
                            output_summary: Some("重新连接后核验成功".into()),
                            error_message: None,
                            finished_at: now_millis(),
                        })
                        .await?;
                }
                self.repository
                    .transition(run_id, OperationStatus::Succeeded)
                    .await?;
            }
            IpRecoveryState::RolledBack => {
                self.restores.mark_remote_rollback_observed(run_id).await?;
                let result = OperationResult {
                    status: OperationConclusion::Warning,
                    summary: "远端超时保护已把网络恢复到修改前状态".into(),
                    findings: Vec::new(),
                    suggestions: vec!["确认旧地址连接正常后，再重新发起 IP 修改。".into()],
                    technical_details: "reconciled=rolled_back".into(),
                };
                self.repository.set_result(run_id, &result).await?;
                self.repository
                    .transition(run_id, OperationStatus::RolledBack)
                    .await?;
            }
            IpRecoveryState::Armed | IpRecoveryState::Staged => {
                return self.require_details(run_id).await;
            }
        }
        self.require_details(run_id).await
    }

    pub async fn inspect_uncertain(&self, run_id: Uuid) -> AppResult<OperationRecoveryResult> {
        let operation = self.reconcile_ip_change(run_id).await?;
        let restore_point = self
            .restores
            .list_by_run(run_id)
            .await?
            .into_iter()
            .next()
            .map(|details| details.point);
        let state_confirmed = operation.run.status != OperationStatus::Uncertain;
        let (what_happened, next_step) = match operation.run.status {
            OperationStatus::Succeeded => (
                "已重新连接服务器并确认修改后的状态".into(),
                "可以继续使用该服务器；恢复点会保留到手动清理".into(),
            ),
            OperationStatus::RolledBack => (
                "远程超时保护已经把服务器恢复到修改前状态".into(),
                "请确认原地址连接正常，再重新发起修改".into(),
            ),
            _ => (
                "服务器仍在自动恢复保护窗口内，当前状态尚未最终确认".into(),
                "等待自动恢复窗口结束后再次检查，不要重复修改网络".into(),
            ),
        };
        Ok(OperationRecoveryResult {
            operation,
            what_happened,
            server_may_have_changed: !state_confirmed,
            state_confirmed,
            next_step,
            restore_point,
            technical_details: None,
        })
    }

    pub async fn rollback_operation(
        &self,
        restore_point_id: Uuid,
    ) -> AppResult<OperationRecoveryResult> {
        let restore = self
            .restores
            .get(restore_point_id)
            .await?
            .ok_or_else(|| AppError::Validation("恢复点不存在".into()))?;
        if !matches!(
            restore.point.status,
            OperationRestorePointStatus::Available
                | OperationRestorePointStatus::Partial
                | OperationRestorePointStatus::Failed
        ) {
            return Err(AppError::RestorePointAlreadyConsumed);
        }
        let run_id = restore.point.operation_run_id;
        let operation = self.require_details(run_id).await?;
        match operation.run.status {
            OperationStatus::Succeeded
            | OperationStatus::Failed
            | OperationStatus::Uncertain
            | OperationStatus::RollbackPartial
            | OperationStatus::RollbackFailed => {
                self.repository
                    .transition(run_id, OperationStatus::RollbackAvailable)
                    .await?;
            }
            OperationStatus::RollbackAvailable => {}
            _ => {
                return Err(AppError::Validation("当前运维状态不允许手动回滚".into()));
            }
        }
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
        let (step_status, run_status, what_happened, next_step, technical_details) = match rollback
        {
            Ok(details) if details.point.status == OperationRestorePointStatus::RolledBack => (
                OperationStepStatus::Succeeded,
                OperationStatus::RolledBack,
                "已按恢复点还原修改前状态".into(),
                "请重新检查对应服务或配置是否恢复正常".into(),
                None,
            ),
            Ok(details) => (
                OperationStepStatus::Failed,
                OperationStatus::RollbackPartial,
                "只完成了部分恢复，服务器可能仍有变化".into(),
                "请停止继续修改，并查看恢复点明细后人工处理".into(),
                Some(format!("restoreStatus={}", details.point.status.as_str())),
            ),
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
                    if uncertain {
                        "回滚期间连接中断，无法确认服务器最终状态".into()
                    } else {
                        "恢复点回滚失败，服务器可能仍有变化".into()
                    },
                    "不要重复执行修改；请先重新连接并检查服务器状态".into(),
                    Some(cap_text(error.to_string(), 8 * 1024)),
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
                    .then(|| "恢复点已经应用并核验".into()),
                error_message: technical_details.clone(),
                finished_at: now_millis(),
            })
            .await?;
        self.repository.transition(run_id, run_status).await?;
        let operation = self.require_details(run_id).await?;
        let restore_point = self
            .restores
            .get(restore_point_id)
            .await?
            .map(|details| details.point);
        Ok(OperationRecoveryResult {
            server_may_have_changed: run_status != OperationStatus::RolledBack,
            state_confirmed: run_status == OperationStatus::RolledBack,
            operation,
            what_happened,
            next_step,
            restore_point,
            technical_details,
        })
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
        let server = self.connector.require_server(server_id).await?;
        if definition.id == "security.firewall_open_port"
            && validated
                .get("action")
                .and_then(|parameter| parameter.value.as_str())
                == Some("remove")
            && validated
                .get("protocol")
                .and_then(|parameter| parameter.value.as_str())
                == Some("tcp")
            && validated
                .get("port")
                .and_then(|parameter| parameter.value.as_u64())
                == Some(u64::from(server.port))
        {
            return Err(AppError::Validation(
                "不能移除当前连接正在使用的 SSH 端口规则，请先迁移 SSH 端口并验证新连接".into(),
            ));
        }
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
        context: OperationPreviewContext,
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
        let target_state_summary = parameter_summary(&plan.parameters)
            .unwrap_or_else(|| "此任务没有需要填写的目标参数".into());
        let backup_summary = plan
            .backup_plan
            .as_ref()
            .map(|backup| {
                backup
                    .items
                    .iter()
                    .map(|item| format!("执行前备份：{}", item.id))
                    .collect()
            })
            .unwrap_or_default();
        let disconnect_risk = disconnect_risk(definition, &plan.parameters);
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
            confirmation_token: (summary.risk_level == RiskLevel::Dangerous).then_some(run.id),
            server: context.server.into(),
            permission_summary: context.permission_summary,
            current_state_summary: context.current_state_summary,
            target_state_summary,
            backup_summary,
            disconnect_risk,
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

fn cap_text(mut value: String, limit: usize) -> String {
    if value.len() <= limit {
        return value;
    }
    let mut boundary = limit;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    value
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

fn required_parameter_str<'a>(
    parameters: &'a ValidatedParameters,
    name: &str,
) -> AppResult<&'a str> {
    parameters
        .get(name)
        .and_then(|parameter| parameter.value.as_str())
        .ok_or_else(|| AppError::Validation(format!("缺少受保护网络参数：{name}")))
}

fn required_parameter_u64(parameters: &ValidatedParameters, name: &str) -> AppResult<u64> {
    parameters
        .get(name)
        .and_then(|parameter| parameter.value.as_u64())
        .ok_or_else(|| AppError::Validation(format!("缺少受保护网络参数：{name}")))
}

fn parse_network_parameter_summary(summary: &str) -> AppResult<Value> {
    let mut object = serde_json::Map::new();
    for item in summary.split(", ") {
        let (name, value) = item
            .split_once('=')
            .ok_or_else(|| AppError::Integrity("IP 修改运行参数摘要格式无效".into()))?;
        if !matches!(name, "interface" | "cidr" | "gateway" | "rollbackSeconds")
            || object.contains_key(name)
        {
            return Err(AppError::Integrity(
                "IP 修改运行参数摘要包含未知或重复字段".into(),
            ));
        }
        let value = if name == "rollbackSeconds" {
            Value::from(
                value
                    .parse::<u64>()
                    .map_err(|_| AppError::Integrity("自动恢复等待时间无效".into()))?,
            )
        } else {
            Value::from(value)
        };
        object.insert(name.into(), value);
    }
    if object.len() != 4 {
        return Err(AppError::Integrity("IP 修改运行参数摘要不完整".into()));
    }
    Ok(Value::Object(object))
}

async fn run_dangerous_preview(
    session: &crate::core::ssh::transport::AuthenticatedSshSession,
    steps: &[RenderedTaskStep],
    privilege_mode: PrivilegeMode,
    redactor: &Redactor,
) -> AppResult<String> {
    let mut summary = String::new();
    for step in steps {
        let command = elevate_fixed_command(&step.command, privilege_mode)?;
        let output = tokio::time::timeout(
            Duration::from_secs(step.timeout_seconds),
            execute_authenticated(session, &command),
        )
        .await
        .map_err(|_| AppError::RemoteStateUncertain("危险任务只读预演超时".into()))??;
        if output.exit_status != 0 {
            return Err(AppError::ssh_command(
                output.exit_status,
                redactor.redact(&output.stderr),
            ));
        }
        append_capped_summary(&mut summary, &redactor.redact(&output.stdout), 8 * 1024);
    }
    if summary.trim().is_empty() {
        Ok("预演检查通过，未返回额外状态信息".into())
    } else {
        Ok(summary)
    }
}

fn append_capped_summary(summary: &mut String, value: &str, limit: usize) {
    if summary.len() >= limit {
        return;
    }
    if !summary.is_empty() {
        summary.push('\n');
    }
    let remaining = limit.saturating_sub(summary.len());
    if value.len() <= remaining {
        summary.push_str(value.trim());
        return;
    }
    let mut boundary = remaining;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    summary.push_str(value[..boundary].trim());
}

fn permission_summary(requirement: PrivilegeRequirement, mode: Option<PrivilegeMode>) -> String {
    match (requirement, mode) {
        (PrivilegeRequirement::CurrentUser, _) => "使用当前 SSH 用户权限".into(),
        (PrivilegeRequirement::RootOrPasswordlessSudo, Some(PrivilegeMode::Root)) => {
            "服务器当前账号为 root".into()
        }
        (PrivilegeRequirement::RootOrPasswordlessSudo, Some(PrivilegeMode::PasswordlessSudo)) => {
            "服务器已配置免密 sudo；客户端不会接收 sudo 密码".into()
        }
        (PrivilegeRequirement::RootOrPasswordlessSudo, None) => {
            "执行前必须确认 root 或免密 sudo 权限".into()
        }
    }
}

fn disconnect_risk(
    definition: &TaskDefinition,
    parameters: &ValidatedParameters,
) -> OperationDisconnectRisk {
    if definition.id == "network.ip_change" {
        let seconds = parameters
            .get("rollbackSeconds")
            .and_then(|parameter| parameter.value.as_u64());
        return OperationDisconnectRisk {
            may_disconnect: true,
            explanation: Some(
                "修改服务器 IP 可能中断当前连接；客户端会先安排远程超时自动恢复".into(),
            ),
            automatic_recovery_seconds: seconds,
        };
    }
    OperationDisconnectRisk {
        may_disconnect: false,
        explanation: None,
        automatic_recovery_seconds: None,
    }
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
