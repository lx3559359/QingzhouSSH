use crate::core::tasks::model::{
    ExecutionScope, OutputKind, ParameterDefinition, PrivilegeRequirement, RiskLevel, TaskCategory,
    TaskDefinition, TaskImplementation, TaskStep,
};

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
