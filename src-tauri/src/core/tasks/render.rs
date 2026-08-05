use crate::{
    core::tasks::{model::TaskImplementation, parameters::ValidatedParameters},
    error::{AppError, AppResult},
};

pub fn render_command(
    implementation: &TaskImplementation,
    parameters: &ValidatedParameters,
) -> AppResult<String> {
    let mut command = implementation
        .execution_steps
        .first()
        .ok_or_else(|| {
            AppError::Validation(format!("任务实现 {} 没有可执行步骤", implementation.id))
        })?
        .command_template
        .clone();
    for (name, parameter) in parameters.iter() {
        command = command.replace(&format!("{{{{{name}}}}}"), &parameter.shell_value);
    }
    if command.contains("{{") || command.contains("}}") {
        return Err(AppError::Validation(format!(
            "任务实现 {} 包含未解析参数",
            implementation.id
        )));
    }
    Ok(command)
}
