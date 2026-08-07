use serde::{Deserialize, Serialize};

use crate::core::{
    system_probe::SystemCapabilities,
    tasks::{remediation_for, TaskDefinition, TaskImplementation},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskAvailabilityState {
    Ready,
    Remediable,
    PermissionBlocked,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAvailabilityEvaluation {
    pub state: TaskAvailabilityState,
    pub implementation_id: Option<String>,
    pub summary: String,
    pub missing_commands: Vec<String>,
    pub blocking_capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRemediationSummary {
    pub package_manager: String,
    pub missing_commands: Vec<String>,
    pub packages: Vec<String>,
}

#[derive(Debug)]
struct ImplementationFit<'a> {
    implementation: &'a TaskImplementation,
    missing_commands: Vec<String>,
    blocking_capabilities: Vec<String>,
}

impl ImplementationFit<'_> {
    fn score(&self) -> usize {
        self.missing_commands.len() + self.blocking_capabilities.len()
    }
}

pub fn evaluate_task_availability(
    definition: &TaskDefinition,
    capabilities: &SystemCapabilities,
) -> TaskAvailabilityEvaluation {
    let best = definition
        .implementations
        .iter()
        .map(|implementation| evaluate_implementation(implementation, capabilities))
        .min_by_key(ImplementationFit::score);

    let Some(best) = best else {
        return TaskAvailabilityEvaluation {
            state: TaskAvailabilityState::Unsupported,
            implementation_id: None,
            summary: "该工具尚未提供可执行实现".into(),
            missing_commands: Vec::new(),
            blocking_capabilities: vec!["没有可用实现".into()],
        };
    };

    if best.score() == 0 {
        return TaskAvailabilityEvaluation {
            state: TaskAvailabilityState::Ready,
            implementation_id: Some(best.implementation.id.clone()),
            summary: "当前服务器可以直接运行".into(),
            missing_commands: Vec::new(),
            blocking_capabilities: Vec::new(),
        };
    }

    let remediation = if best.blocking_capabilities.is_empty() {
        remediation_for(
            capabilities.package_manager.as_deref(),
            &best.missing_commands,
        )
    } else {
        None
    };
    let state = if remediation.is_some() {
        TaskAvailabilityState::Remediable
    } else {
        TaskAvailabilityState::Unsupported
    };
    let summary = if let Some(remediation) = &remediation {
        format!(
            "缺少组件 {}，确认后可通过 {} 安装 {}",
            remediation.missing_commands.join("、"),
            remediation.package_manager,
            remediation.packages.join("、")
        )
    } else {
        unavailable_summary(&best.missing_commands, &best.blocking_capabilities)
    };
    TaskAvailabilityEvaluation {
        state,
        implementation_id: Some(best.implementation.id.clone()),
        summary,
        missing_commands: best.missing_commands,
        blocking_capabilities: best.blocking_capabilities,
    }
}

fn evaluate_implementation<'a>(
    implementation: &'a TaskImplementation,
    capabilities: &SystemCapabilities,
) -> ImplementationFit<'a> {
    let predicate = &implementation.compatibility;
    let mut blocking_capabilities = Vec::new();
    if !predicate.os_families.is_empty()
        && !predicate
            .os_families
            .iter()
            .any(|family| family == &capabilities.os_family)
    {
        blocking_capabilities.push(format!(
            "系统类型 {} 不在该实现支持范围内",
            capabilities.os_family
        ));
    }
    if !predicate.service_managers.is_empty()
        && !predicate
            .service_managers
            .iter()
            .any(|manager| manager == &capabilities.service_manager)
    {
        blocking_capabilities.push(format!(
            "服务管理器 {} 不受该实现支持",
            capabilities.service_manager
        ));
    }

    let mut missing_commands = predicate
        .required_commands
        .iter()
        .filter(|command| !capabilities.has_command(command))
        .cloned()
        .collect::<Vec<_>>();
    missing_commands.sort();
    missing_commands.dedup();

    ImplementationFit {
        implementation,
        missing_commands,
        blocking_capabilities,
    }
}

fn unavailable_summary(missing_commands: &[String], blockers: &[String]) -> String {
    let mut parts = Vec::new();
    if !missing_commands.is_empty() {
        parts.push(format!("服务器缺少组件：{}", missing_commands.join("、")));
    }
    if !blockers.is_empty() {
        parts.push(blockers.join("；"));
    }
    if parts.is_empty() {
        "当前服务器暂不支持该工具".into()
    } else {
        parts.join("；")
    }
}
