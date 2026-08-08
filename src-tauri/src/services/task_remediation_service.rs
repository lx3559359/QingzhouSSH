use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    core::{
        ssh::executor::EventSink,
        tasks::{
            built_in_catalog, elevate_fixed_command, evaluate_task_availability,
            fixed_install_command, probe_privilege, remediation_for, PackageId, PackageManagerKind,
            PrivilegeMode, TaskAvailabilityState,
        },
    },
    domain::execution::now_millis,
    error::{AppError, AppResult},
    services::{
        execution_service::{ExecutionService, TaskAvailability},
        server_connector::ServerConnector,
    },
};

const PREVIEW_TTL_MILLIS: i64 = 5 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemediationBinding {
    pub server_id: String,
    pub task_id: String,
    pub implementation_id: String,
    pub missing_commands: Vec<String>,
    pub packages: Vec<String>,
    pub package_manager: String,
    pub privilege_mode: PrivilegeMode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRemediationPreview {
    pub preview_id: Uuid,
    pub confirmation_token: Uuid,
    pub expires_at: i64,
    pub task_id: String,
    pub implementation_id: String,
    pub missing_commands: Vec<String>,
    pub packages: Vec<String>,
    pub package_manager: String,
    pub permission_state: TaskAvailabilityState,
    pub command_summary: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfirmTaskRemediationRequest {
    pub preview_id: Uuid,
    pub confirmation_token: Uuid,
}

#[derive(Debug, Clone)]
struct StoredRemediationPreview {
    preview: TaskRemediationPreview,
    binding: RemediationBinding,
}

#[derive(Clone, Default)]
pub struct RemediationPreviewRegistry {
    previews: Arc<Mutex<HashMap<Uuid, StoredRemediationPreview>>>,
}

impl RemediationPreviewRegistry {
    pub async fn issue(
        &self,
        binding: RemediationBinding,
        now: i64,
    ) -> AppResult<TaskRemediationPreview> {
        let command_summary = command_for_binding(&binding)?;
        let preview = TaskRemediationPreview {
            preview_id: Uuid::new_v4(),
            confirmation_token: Uuid::new_v4(),
            expires_at: now.saturating_add(PREVIEW_TTL_MILLIS),
            task_id: binding.task_id.clone(),
            implementation_id: binding.implementation_id.clone(),
            missing_commands: binding.missing_commands.clone(),
            packages: binding.packages.clone(),
            package_manager: binding.package_manager.clone(),
            permission_state: TaskAvailabilityState::Ready,
            command_summary,
        };
        self.previews.lock().await.insert(
            preview.preview_id,
            StoredRemediationPreview {
                preview: preview.clone(),
                binding,
            },
        );
        Ok(preview)
    }

    pub async fn consume(
        &self,
        preview_id: Uuid,
        confirmation_token: Uuid,
        now: i64,
    ) -> AppResult<RemediationBinding> {
        let mut previews = self.previews.lock().await;
        let stored = previews
            .get(&preview_id)
            .ok_or_else(|| AppError::Validation("组件补齐预览不存在或已经使用".into()))?;
        if stored.preview.expires_at < now {
            previews.remove(&preview_id);
            return Err(AppError::Validation(
                "组件补齐确认已经过期，请重新预览".into(),
            ));
        }
        if stored.preview.confirmation_token != confirmation_token {
            return Err(AppError::Security("组件补齐确认令牌无效".into()));
        }
        previews
            .remove(&preview_id)
            .map(|stored| stored.binding)
            .ok_or_else(|| AppError::Validation("组件补齐预览已经使用".into()))
    }
}

pub fn ensure_same_binding(
    expected: &RemediationBinding,
    current: &RemediationBinding,
) -> AppResult<()> {
    if expected.server_id != current.server_id
        || expected.task_id != current.task_id
        || expected.implementation_id != current.implementation_id
        || expected.missing_commands != current.missing_commands
        || expected.packages != current.packages
        || expected.package_manager != current.package_manager
    {
        return Err(AppError::Security(
            "服务器能力或组件清单在确认前发生变化，请重新预览".into(),
        ));
    }
    Ok(())
}

#[derive(Clone)]
pub struct TaskRemediationService {
    connector: ServerConnector,
    executions: ExecutionService,
    registry: RemediationPreviewRegistry,
}

impl TaskRemediationService {
    pub fn new(connector: ServerConnector, executions: ExecutionService) -> Self {
        Self {
            connector,
            executions,
            registry: RemediationPreviewRegistry::default(),
        }
    }

    pub async fn preview(
        &self,
        server_id: &str,
        task_id: &str,
    ) -> AppResult<TaskRemediationPreview> {
        let connected = self.connector.connect(server_id).await?;
        let result = async {
            let definition = find_task(task_id)?;
            let binding = binding_for(
                server_id,
                &definition.id,
                &connected.capabilities,
                probe_privilege(&connected.session).await?,
            )?;
            self.registry.issue(binding, now_millis()).await
        }
        .await;
        result
    }

    pub async fn confirm<E: EventSink>(
        &self,
        server_id: &str,
        request: ConfirmTaskRemediationRequest,
        events: &mut E,
    ) -> AppResult<TaskAvailability> {
        let expected = self
            .registry
            .consume(request.preview_id, request.confirmation_token, now_millis())
            .await?;
        if expected.server_id != server_id {
            return Err(AppError::Security("确认令牌不属于当前服务器".into()));
        }

        let connected = self.connector.connect(server_id).await?;
        let current_result = async {
            let privilege_mode = probe_privilege(&connected.session).await?;
            binding_for(
                server_id,
                &expected.task_id,
                &connected.capabilities,
                privilege_mode,
            )
        }
        .await;
        let current = current_result?;
        ensure_same_binding(&expected, &current)?;

        let command =
            elevate_fixed_command(&command_for_binding(&current)?, current.privilege_mode)?;
        let execution_result = self
            .executions
            .execute_fixed_maintenance(
                server_id,
                &current.package_manager,
                &current.packages,
                command,
                events,
            )
            .await;
        let refreshed = self.executions.list_task_definitions(server_id).await;
        execution_result?;
        refreshed?
            .into_iter()
            .find(|availability| availability.definition.id == current.task_id)
            .ok_or_else(|| AppError::Validation("补齐后无法重新读取目标工具".into()))
    }
}

fn find_task(task_id: &str) -> AppResult<crate::core::tasks::TaskDefinition> {
    built_in_catalog()
        .into_iter()
        .find(|definition| definition.id == task_id)
        .ok_or_else(|| AppError::Validation("要补齐组件的工具不存在".into()))
}

fn binding_for(
    server_id: &str,
    task_id: &str,
    capabilities: &crate::core::system_probe::SystemCapabilities,
    privilege_mode: PrivilegeMode,
) -> AppResult<RemediationBinding> {
    let definition = find_task(task_id)?;
    let evaluation = evaluate_task_availability(&definition, capabilities);
    if evaluation.state != TaskAvailabilityState::Remediable {
        return Err(AppError::Validation(evaluation.summary));
    }
    let remediation = remediation_for(
        capabilities.package_manager.as_deref(),
        &evaluation.missing_commands,
    )
    .ok_or_else(|| AppError::Security("缺失组件不在固定补齐白名单中".into()))?;
    Ok(RemediationBinding {
        server_id: server_id.into(),
        task_id: task_id.into(),
        implementation_id: evaluation
            .implementation_id
            .ok_or_else(|| AppError::Compatibility("工具没有可补齐的实现".into()))?,
        missing_commands: remediation.missing_commands,
        packages: remediation.packages,
        package_manager: remediation.package_manager,
        privilege_mode,
    })
}

fn command_for_binding(binding: &RemediationBinding) -> AppResult<String> {
    let manager = PackageManagerKind::try_from(binding.package_manager.as_str())?;
    let packages = binding
        .packages
        .iter()
        .map(|package| PackageId::try_from(package.as_str()))
        .collect::<AppResult<Vec<_>>>()?;
    fixed_install_command(manager, &packages)
}
