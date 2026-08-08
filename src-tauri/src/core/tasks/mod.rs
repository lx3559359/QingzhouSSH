mod availability;
mod catalog;
mod library;
mod model;
mod parameters;
mod planner;
mod privilege;
mod recovery;
mod remediation;
mod render;
mod result;

pub use availability::{
    evaluate_task_availability, TaskAvailabilityEvaluation, TaskAvailabilityState,
    TaskRemediationSummary,
};
pub use catalog::built_in_catalog;
pub use library::{metadata_for, ToolLibraryCategory, ToolLibraryMetadata, ToolSource};
pub use model::{
    BackupItemDefinition, BackupItemKind, BackupPlan, CompatibilityPredicate, ExecutionScope,
    OutputKind, ParameterDefinition, ParameterKind, PrivilegeMode, PrivilegeRequirement,
    ResultParserKind, RiskLevel, RollbackPlan, TaskCategory, TaskDefinition, TaskImplementation,
    TaskStep,
};
pub use parameters::{
    script_parameter_env_name, shell_quote, validate_parameters, ValidatedParameter,
    ValidatedParameters,
};
pub use planner::{plan_task, validate_scope, PlannedTask, PublicTaskPlan, RenderedTaskStep};
pub use privilege::{
    elevate_fixed_command, evaluate_privilege_probe, probe_privilege,
    PASSWORDLESS_SUDO_PROBE_COMMAND, PRIVILEGE_UID_COMMAND,
};
pub(crate) use recovery::validate_confined_relative_path;
pub(crate) use recovery::{prepare_task_restore_destination, render_backup_target};
pub use recovery::{
    resolve_task_restore_path, task_restore_dir, task_restore_item_relative_path,
    validate_restore_relative_path, write_restore_asset_atomic, StoredRestoreAsset,
};
pub use remediation::{fixed_install_command, remediation_for, PackageId, PackageManagerKind};
pub use render::render_command;
pub use result::{
    parse_result, FindingLevel, OperationConclusion, OperationFinding, OperationResult,
};

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
        || predicate.os_families.iter().any(|family| {
            family == &capabilities.os_family || family == capabilities.platform_family.as_str()
        }))
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

#[cfg(test)]
mod platform_adapter_tests {
    use super::*;
    use crate::core::system_probe::{RemoteOsFamily, RemotePathStyle, RemoteShell};

    fn task(id: &str) -> TaskDefinition {
        built_in_catalog()
            .into_iter()
            .find(|definition| definition.id == id)
            .unwrap()
    }

    #[test]
    fn selects_fixed_windows_powershell_adapters_for_safe_tasks() {
        let capabilities = SystemCapabilities {
            platform_family: RemoteOsFamily::Windows,
            remote_shell: RemoteShell::PowerShell,
            path_style: RemotePathStyle::WindowsSftp,
            os_id: "windows".into(),
            os_family: "windows".into(),
            service_manager: "windows_service_control_manager".into(),
            commands: vec![
                "powershell".into(),
                "get-ciminstance".into(),
                "get-process".into(),
            ],
            ..Default::default()
        };

        for id in ["system.overview", "system.disk_usage", "service.inventory"] {
            let definition = task(id);
            let implementation = select_implementation(&definition, &capabilities).unwrap();
            assert!(implementation.id.starts_with("windows-powershell"));
            assert!(implementation.execution_steps[0]
                .command_template
                .starts_with("powershell.exe -NoLogo -NoProfile -NonInteractive"));
            assert!(implementation.execution_steps[0]
                .command_template
                .contains("-EncodedCommand"));
        }
    }

    #[test]
    fn selects_bsd_adapters_instead_of_linux_command_variants() {
        let capabilities = SystemCapabilities {
            platform_family: RemoteOsFamily::Bsd,
            remote_shell: RemoteShell::PosixSh,
            path_style: RemotePathStyle::Posix,
            os_id: "freebsd".into(),
            os_family: "bsd".into(),
            service_manager: "service".into(),
            commands: vec![
                "sh".into(),
                "uname".into(),
                "uptime".into(),
                "df".into(),
                "head".into(),
                "ps".into(),
                "sysctl".into(),
                "service".into(),
            ],
            ..Default::default()
        };

        assert_eq!(
            select_implementation(&task("system.overview"), &capabilities)
                .unwrap()
                .id,
            "bsd"
        );
        assert_eq!(
            select_implementation(&task("service.inventory"), &capabilities)
                .unwrap()
                .id,
            "bsd-service"
        );
    }
}
