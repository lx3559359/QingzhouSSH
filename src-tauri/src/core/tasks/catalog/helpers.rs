use crate::core::tasks::model::{
    CompatibilityPredicate, ExecutionScope, OutputKind, ParameterDefinition, ParameterKind,
    PrivilegeRequirement, ResultParserKind, RiskLevel, TaskCategory, TaskDefinition,
    TaskImplementation, TaskStep,
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
    TaskImplementation {
        id: id.into(),
        compatibility: CompatibilityPredicate {
            os_families: SUPPORTED_FAMILIES
                .iter()
                .map(|value| (*value).into())
                .collect(),
            service_managers: Vec::new(),
            required_commands: required_commands
                .iter()
                .map(|value| (*value).into())
                .collect(),
        },
        preflight_steps: Vec::new(),
        preview_steps: Vec::new(),
        backup_plan: None,
        execution_steps: steps,
        verify_steps: Vec::new(),
        rollback_plan: None,
        result_parser,
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
