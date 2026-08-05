use serde::Serialize;
use serde_json::Value;

use crate::{
    core::{
        system_probe::SystemCapabilities,
        tasks::{
            model::{
                ExecutionScope, ParameterKind, PrivilegeRequirement, ResultParserKind, RiskLevel,
                TaskDefinition, TaskStep,
            },
            parameters::{validate_parameters, ValidatedParameters},
            render::render_task_step_command,
            select_implementation,
        },
    },
    error::{AppError, AppResult},
};

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedTask {
    pub definition_id: String,
    pub definition_version: i32,
    pub implementation_id: String,
    pub risk_level: RiskLevel,
    pub privilege: PrivilegeRequirement,
    pub parameters: ValidatedParameters,
    pub preflight_steps: Vec<RenderedTaskStep>,
    pub execution_steps: Vec<RenderedTaskStep>,
    pub result_parser: ResultParserKind,
    estimated_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedTaskStep {
    pub id: String,
    pub title: String,
    pub command: String,
    pub timeout_seconds: u64,
    pub output_limit_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicTaskPlan {
    pub definition_id: String,
    pub definition_version: i32,
    pub implementation_id: String,
    pub risk_level: RiskLevel,
    pub privilege: PrivilegeRequirement,
    pub step_titles: Vec<String>,
    pub estimated_seconds: u32,
}

impl PlannedTask {
    pub fn public_summary(&self) -> PublicTaskPlan {
        PublicTaskPlan {
            definition_id: self.definition_id.clone(),
            definition_version: self.definition_version,
            implementation_id: self.implementation_id.clone(),
            risk_level: self.risk_level,
            privilege: self.privilege,
            step_titles: self
                .preflight_steps
                .iter()
                .chain(&self.execution_steps)
                .map(|step| step.title.clone())
                .collect(),
            estimated_seconds: self.estimated_seconds,
        }
    }
}

pub fn plan_task(
    definition: &TaskDefinition,
    capabilities: &SystemCapabilities,
    input: &Value,
) -> AppResult<PlannedTask> {
    let parameters = validate_parameters(definition, input)?;
    validate_discovered_targets(definition, capabilities, &parameters)?;
    let implementation = select_implementation(definition, capabilities)?;
    let preflight_steps = render_steps(
        &implementation.preflight_steps,
        &implementation.id,
        &parameters,
    )?;
    let execution_steps = render_steps(
        &implementation.execution_steps,
        &implementation.id,
        &parameters,
    )?;
    if execution_steps.is_empty() {
        return Err(AppError::Validation(format!(
            "任务实现 {} 没有可执行步骤",
            implementation.id
        )));
    }

    Ok(PlannedTask {
        definition_id: definition.id.clone(),
        definition_version: definition.version,
        implementation_id: implementation.id.clone(),
        risk_level: definition.risk_level,
        privilege: definition.privilege,
        parameters,
        preflight_steps,
        execution_steps,
        result_parser: implementation.result_parser,
        estimated_seconds: definition.estimated_seconds,
    })
}

fn validate_discovered_targets(
    definition: &TaskDefinition,
    capabilities: &SystemCapabilities,
    parameters: &ValidatedParameters,
) -> AppResult<()> {
    for parameter in &definition.parameters {
        let Some(value) = parameters
            .get(&parameter.name)
            .and_then(|parameter| parameter.value.as_str())
        else {
            continue;
        };
        let available = match &parameter.kind {
            ParameterKind::ServiceName => capabilities.has_service(value),
            ParameterKind::ContainerName => capabilities.has_container(value),
            _ => continue,
        };
        if !available {
            return Err(AppError::Compatibility(format!(
                "目标 {} 未在服务器能力探测结果中发现",
                parameter.label
            )));
        }
    }
    Ok(())
}

pub fn validate_scope(definition: &TaskDefinition, server_count: usize) -> AppResult<()> {
    if server_count == 0 {
        return Err(AppError::Validation("至少需要选择一台服务器".into()));
    }
    if server_count > 1
        && (definition.risk_level != RiskLevel::Safe
            || definition.scope != ExecutionScope::ReadOnlyBatch)
    {
        return Err(AppError::Validation(
            "只有安全的只读任务可以批量运行".into(),
        ));
    }
    Ok(())
}

fn render_steps(
    steps: &[TaskStep],
    implementation_id: &str,
    parameters: &ValidatedParameters,
) -> AppResult<Vec<RenderedTaskStep>> {
    steps
        .iter()
        .map(|step| {
            Ok(RenderedTaskStep {
                id: step.id.clone(),
                title: step.title.clone(),
                command: render_task_step_command(step, implementation_id, parameters)?,
                timeout_seconds: step.timeout_seconds,
                output_limit_bytes: step.output_limit_bytes,
            })
        })
        .collect()
}
