use crate::core::tasks::model::{
    BackupItemDefinition, BackupItemKind, BackupPlan, CompatibilityPredicate, ExecutionScope,
    OutputKind, ParameterDefinition, ParameterKind, PrivilegeRequirement, ResultParserKind,
    RiskLevel, RollbackPlan, TaskCategory, TaskDefinition, TaskImplementation, TaskStep,
};
use serde_json::{json, Value};

const SUPPORTED_FAMILIES: [&str; 3] = ["debian", "rhel", "openeuler"];

pub(super) fn read_only_task(
    id: &str,
    category: TaskCategory,
    title: &str,
    description: &str,
    estimated_seconds: u32,
    parameters: Vec<ParameterDefinition>,
    implementations: Vec<TaskImplementation>,
) -> TaskDefinition {
    TaskDefinition {
        id: id.into(),
        version: 2,
        category,
        title: title.into(),
        description: description.into(),
        risk_level: RiskLevel::Safe,
        estimated_seconds,
        privilege: PrivilegeRequirement::CurrentUser,
        scope: ExecutionScope::ReadOnlyBatch,
        parameters,
        implementations,
        output_kind: OutputKind::KeyValue,
    }
}

pub(super) fn bounded_step(id: &str, title: &str, seconds: u64, template: &str) -> TaskStep {
    TaskStep {
        id: id.into(),
        title: title.into(),
        timeout_seconds: seconds,
        output_limit_bytes: 1024 * 1024,
        command_template: template.into(),
    }
}

pub(super) fn read_only_implementation(
    id: &str,
    required_commands: &[&str],
    steps: Vec<TaskStep>,
    result_parser: ResultParserKind,
) -> TaskImplementation {
    let preview_steps = steps.clone();
    TaskImplementation {
        id: id.into(),
        compatibility: CompatibilityPredicate {
            os_families: Vec::new(),
            service_managers: Vec::new(),
            required_commands: required_commands
                .iter()
                .map(|value| (*value).into())
                .collect(),
        },
        preflight_steps: Vec::new(),
        preview_steps,
        backup_plan: None,
        execution_steps: steps,
        verify_steps: Vec::new(),
        rollback_plan: None,
        result_parser,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn dangerous_task(
    id: &str,
    category: TaskCategory,
    title: &str,
    description: &str,
    estimated_seconds: u32,
    parameters: Vec<ParameterDefinition>,
    implementations: Vec<TaskImplementation>,
    output_kind: OutputKind,
) -> TaskDefinition {
    TaskDefinition {
        id: id.into(),
        version: 2,
        category,
        title: title.into(),
        description: description.into(),
        risk_level: RiskLevel::Dangerous,
        estimated_seconds,
        privilege: PrivilegeRequirement::RootOrPasswordlessSudo,
        scope: ExecutionScope::SingleServer,
        parameters,
        implementations,
        output_kind,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn dangerous_implementation(
    id: &str,
    service_managers: &[&str],
    required_commands: &[&str],
    preview_command: &str,
    backup_items: Vec<BackupItemDefinition>,
    execution_command: &str,
    verify_command: &str,
    rollback_command: &str,
    result_parser: ResultParserKind,
) -> TaskImplementation {
    TaskImplementation {
        id: id.into(),
        compatibility: CompatibilityPredicate {
            os_families: SUPPORTED_FAMILIES
                .iter()
                .map(|value| (*value).into())
                .collect(),
            service_managers: service_managers
                .iter()
                .map(|value| (*value).into())
                .collect(),
            required_commands: required_commands
                .iter()
                .map(|value| (*value).into())
                .collect(),
        },
        preflight_steps: vec![bounded_step(
            "preflight",
            "检查目标与恢复条件",
            30,
            preview_command,
        )],
        preview_steps: vec![bounded_step("preview", "预演当前状态", 30, preview_command)],
        backup_plan: Some(BackupPlan {
            items: backup_items,
        }),
        execution_steps: vec![bounded_step(
            "execute",
            "执行受控修改",
            60,
            execution_command,
        )],
        verify_steps: vec![bounded_step("verify", "核验目标状态", 45, verify_command)],
        rollback_plan: Some(RollbackPlan {
            steps: vec![bounded_step(
                "rollback",
                "恢复修改前状态",
                60,
                rollback_command,
            )],
            automatic_on_failure: true,
        }),
        result_parser,
    }
}

pub(super) fn backup_item(
    id: &str,
    kind: BackupItemKind,
    target_template: &str,
) -> BackupItemDefinition {
    BackupItemDefinition {
        id: id.into(),
        kind,
        target_template: target_template.into(),
    }
}

pub(super) fn integer_parameter(
    name: &str,
    label: &str,
    description: &str,
    min: i64,
    max: i64,
    default: i64,
) -> ParameterDefinition {
    parameter(
        name,
        label,
        description,
        ParameterKind::Integer { min, max },
        false,
        Some(json!(default)),
    )
}

pub(super) fn string_parameter(
    name: &str,
    label: &str,
    description: &str,
    min_length: usize,
    max_length: usize,
) -> ParameterDefinition {
    parameter(
        name,
        label,
        description,
        ParameterKind::String {
            min_length,
            max_length,
            multiline: false,
        },
        true,
        None,
    )
}

pub(super) fn absolute_path_parameter(
    name: &str,
    label: &str,
    description: &str,
) -> ParameterDefinition {
    parameter(
        name,
        label,
        description,
        ParameterKind::AbsolutePath,
        true,
        None,
    )
}

pub(super) fn host_parameter(
    name: &str,
    label: &str,
    description: &str,
    required: bool,
) -> ParameterDefinition {
    parameter(
        name,
        label,
        description,
        ParameterKind::Host,
        required,
        None,
    )
}

pub(super) fn port_parameter(
    name: &str,
    label: &str,
    description: &str,
    required: bool,
    default: Option<u16>,
) -> ParameterDefinition {
    parameter(
        name,
        label,
        description,
        ParameterKind::Port,
        required,
        default.map(|value| json!(value)),
    )
}

pub(super) fn interface_parameter(
    name: &str,
    label: &str,
    description: &str,
) -> ParameterDefinition {
    parameter(
        name,
        label,
        description,
        ParameterKind::InterfaceName,
        true,
        None,
    )
}

pub(super) fn boolean_parameter(
    name: &str,
    label: &str,
    description: &str,
    default: bool,
) -> ParameterDefinition {
    parameter(
        name,
        label,
        description,
        ParameterKind::Boolean,
        false,
        Some(json!(default)),
    )
}

pub(super) fn service_parameter() -> ParameterDefinition {
    parameter(
        "service",
        "服务",
        "从服务器已发现的服务中选择",
        ParameterKind::ServiceName,
        true,
        None,
    )
}

pub(super) fn service_multi_parameter(max_items: usize) -> ParameterDefinition {
    parameter(
        "services",
        "服务",
        "从服务器已发现的服务中选择一个或多个",
        ParameterKind::ServiceMultiSelect { max_items },
        true,
        None,
    )
}

pub(super) fn container_parameter() -> ParameterDefinition {
    parameter(
        "container",
        "容器",
        "从服务器已发现的容器中选择",
        ParameterKind::ContainerName,
        true,
        None,
    )
}

pub(super) fn enum_parameter(
    name: &str,
    label: &str,
    description: &str,
    options: &[&str],
    default: Option<&str>,
) -> ParameterDefinition {
    parameter(
        name,
        label,
        description,
        ParameterKind::Enum {
            options: options.iter().map(|value| (*value).into()).collect(),
        },
        default.is_none(),
        default.map(|value| json!(value)),
    )
}

pub(super) fn parameter(
    name: &str,
    label: &str,
    description: &str,
    kind: ParameterKind,
    required: bool,
    default_value: Option<Value>,
) -> ParameterDefinition {
    ParameterDefinition {
        name: name.into(),
        label: label.into(),
        description: description.into(),
        kind,
        required,
        default_value,
        sensitive: false,
    }
}
