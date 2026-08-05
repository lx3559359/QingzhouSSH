mod catalog;
mod model;
mod parameters;
mod render;

pub use catalog::built_in_catalog;
pub use model::{
    BackupItemDefinition, BackupItemKind, BackupPlan, CompatibilityPredicate, ExecutionScope,
    OutputKind, ParameterDefinition, ParameterKind, PrivilegeRequirement, ResultParserKind,
    RiskLevel, RollbackPlan, TaskCategory, TaskDefinition, TaskImplementation, TaskStep,
};
pub use parameters::{shell_quote, validate_parameters, ValidatedParameter, ValidatedParameters};
pub use render::render_command;

use crate::{
    core::system_probe::SystemCapabilities,
    error::{AppError, AppResult},
};

pub fn select_implementation<'a>(
    definition: &'a TaskDefinition,
    capabilities: &SystemCapabilities,
) -> AppResult<&'a TaskImplementation> {
    definition
        .implementations
        .iter()
        .find(|implementation| matches_capabilities(&implementation.compatibility, capabilities))
        .ok_or_else(|| {
            AppError::Compatibility(format!(
                "任务 {} 不支持系统 {} ({}) 或缺少必要命令",
                definition.id, capabilities.os_id, capabilities.os_family
            ))
        })
}

pub fn task_version_is_compatible(definition: &TaskDefinition, requested_version: i32) -> bool {
    definition.version == requested_version
        || (definition.version == 2
            && requested_version == 1
            && matches!(
                definition.id.as_str(),
                "system.overview"
                    | "system.disk_usage"
                    | "system.process_query"
                    | "service.status"
                    | "service.start"
                    | "service.stop"
                    | "service.restart"
                    | "logs.search"
            ))
}

fn matches_capabilities(
    predicate: &CompatibilityPredicate,
    capabilities: &SystemCapabilities,
) -> bool {
    (predicate.os_families.is_empty()
        || predicate
            .os_families
            .iter()
            .any(|family| family == &capabilities.os_family))
        && (predicate.service_managers.is_empty()
            || predicate
                .service_managers
                .iter()
                .any(|manager| manager == &capabilities.service_manager))
        && predicate
            .required_commands
            .iter()
            .all(|command| capabilities.has_command(command))
}
