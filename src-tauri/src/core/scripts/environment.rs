use base64::{engine::general_purpose::STANDARD, Engine as _};
use uuid::Uuid;

use crate::{
    core::{
        scripts::validation::validate_script_body,
        tasks::{script_parameter_env_name, ValidatedParameters},
    },
    domain::script::ScriptShell,
    error::{AppError, AppResult},
};

pub fn render_script_launcher(
    shell: ScriptShell,
    executable: &str,
    body: &str,
    parameters: &ValidatedParameters,
) -> AppResult<String> {
    validate_script_body(body)?;
    match (shell, executable) {
        (ScriptShell::PosixSh, "sh") | (ScriptShell::Bash, "bash") => {
            render_posix_launcher(executable, body, parameters)
        }
        (ScriptShell::PowerShell, "powershell" | "pwsh") => {
            render_powershell_launcher(executable, body, parameters)
        }
        _ => Err(AppError::Validation("脚本 Shell 启动器不匹配".into())),
    }
}

fn render_posix_launcher(
    executable: &str,
    body: &str,
    parameters: &ValidatedParameters,
) -> AppResult<String> {
    let delimiter = unique_heredoc_delimiter(body)?;
    let mut launcher = String::with_capacity(body.len() + parameters.iter().count() * 64 + 96);
    launcher.push_str("env");
    for (name, parameter) in parameters.iter() {
        launcher.push(' ');
        launcher.push_str(&script_parameter_env_name(name)?);
        launcher.push('=');
        launcher.push_str(&parameter.shell_value);
    }
    launcher.push(' ');
    launcher.push_str(executable);
    launcher.push_str(" -s <<'");
    launcher.push_str(&delimiter);
    launcher.push_str("'\n");
    launcher.push_str(body);
    launcher.push('\n');
    launcher.push_str(&delimiter);
    Ok(launcher)
}

fn render_powershell_launcher(
    executable: &str,
    body: &str,
    parameters: &ValidatedParameters,
) -> AppResult<String> {
    let mut script = String::with_capacity(body.len() + parameters.iter().count() * 80 + 64);
    script.push_str("$ErrorActionPreference = 'Stop'\n");
    for (name, parameter) in parameters.iter() {
        let name = script_parameter_env_name(name)?;
        script.push_str("$env:");
        script.push_str(&name);
        script.push_str(" = '");
        script.push_str(&powershell_value(&parameter.value).replace('\'', "''"));
        script.push_str("'\n");
    }
    script.push_str(body);
    let utf16 = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    Ok(format!(
        "{executable} -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
        STANDARD.encode(utf16)
    ))
}

fn powershell_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Null => String::new(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_shell_specific_non_interactive_launchers() {
        let parameters = ValidatedParameters::default();
        let sh =
            render_script_launcher(ScriptShell::PosixSh, "sh", "echo ok", &parameters).unwrap();
        assert!(sh.starts_with("env sh -s <<'QZ_SCRIPT_"));

        let bash =
            render_script_launcher(ScriptShell::Bash, "bash", "set -o pipefail", &parameters)
                .unwrap();
        assert!(bash.starts_with("env bash -s <<'QZ_SCRIPT_"));

        let powershell = render_script_launcher(
            ScriptShell::PowerShell,
            "pwsh",
            "Write-Output 'ok'",
            &parameters,
        )
        .unwrap();
        assert!(powershell.starts_with("pwsh -NoLogo -NoProfile -NonInteractive -EncodedCommand "));
        assert!(!powershell.contains("Write-Output"));
    }

    #[test]
    fn rejects_a_mismatched_shell_launcher() {
        let error = render_script_launcher(
            ScriptShell::PowerShell,
            "bash",
            "Write-Output ok",
            &ValidatedParameters::default(),
        )
        .unwrap_err();
        assert!(matches!(error, AppError::Validation(_)));
    }
}
