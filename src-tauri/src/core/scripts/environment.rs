use uuid::Uuid;

use crate::{
    core::{
        scripts::validation::validate_script_body,
        tasks::{script_parameter_env_name, ValidatedParameters},
    },
    error::{AppError, AppResult},
};

pub fn render_script_launcher(body: &str, parameters: &ValidatedParameters) -> AppResult<String> {
    validate_script_body(body)?;
    let delimiter = unique_heredoc_delimiter(body)?;
    let mut launcher = String::with_capacity(body.len() + parameters.iter().count() * 64 + 96);
    launcher.push_str("env");
    for (name, parameter) in parameters.iter() {
        launcher.push(' ');
        launcher.push_str(&script_parameter_env_name(name)?);
        launcher.push('=');
        launcher.push_str(&parameter.shell_value);
    }
    launcher.push_str(" sh -s <<'");
    launcher.push_str(&delimiter);
    launcher.push_str("'\n");
    launcher.push_str(body);
    launcher.push('\n');
    launcher.push_str(&delimiter);
    Ok(launcher)
}

fn unique_heredoc_delimiter(body: &str) -> AppResult<String> {
    for _ in 0..16 {
        let delimiter = format!("QZ_SCRIPT_{}", Uuid::new_v4().simple());
        if !body.contains(&delimiter) {
            return Ok(delimiter);
        }
    }
    Err(AppError::Integrity(
        "无法为脚本创建安全的输入边界，请重新尝试".into(),
    ))
}
