use crate::{
    core::{
        ssh::transport::{execute_authenticated, AuthenticatedSshSession, CommandOutput},
        tasks::{parameters::shell_quote, PrivilegeMode},
    },
    error::{AppError, AppResult},
};

pub const PRIVILEGE_UID_COMMAND: &str = "id -u";
pub const PASSWORDLESS_SUDO_PROBE_COMMAND: &str = "sudo -n true";

pub async fn probe_privilege(session: &AuthenticatedSshSession) -> AppResult<PrivilegeMode> {
    let uid_output = execute_authenticated(session, PRIVILEGE_UID_COMMAND).await?;
    let uid = parse_uid(&uid_output)?;
    if uid == 0 {
        return Ok(PrivilegeMode::Root);
    }

    let sudo_output = execute_authenticated(session, PASSWORDLESS_SUDO_PROBE_COMMAND).await?;
    evaluate_privilege_probe(&uid_output, Some(&sudo_output))
}

pub fn evaluate_privilege_probe(
    uid_output: &CommandOutput,
    sudo_output: Option<&CommandOutput>,
) -> AppResult<PrivilegeMode> {
    if parse_uid(uid_output)? == 0 {
        return Ok(PrivilegeMode::Root);
    }
    if sudo_output.is_some_and(|output| output.exit_status == 0) {
        return Ok(PrivilegeMode::PasswordlessSudo);
    }
    Err(AppError::PasswordlessSudoRequired)
}

pub fn elevate_fixed_command(command: &str, mode: PrivilegeMode) -> AppResult<String> {
    if command.is_empty() || command.contains('\0') {
        return Err(AppError::Validation(
            "内置提权命令不能为空或包含无效字符".into(),
        ));
    }
    if ["sudo -S", "SUDO_ASKPASS", "--stdin"]
        .iter()
        .any(|forbidden| command.contains(forbidden))
    {
        return Err(AppError::Security(
            "内置命令包含被禁止的交互式提权方式".into(),
        ));
    }
    match mode {
        PrivilegeMode::Root => Ok(command.into()),
        PrivilegeMode::PasswordlessSudo => Ok(format!("sudo -n -- sh -c {}", shell_quote(command))),
    }
}

fn parse_uid(output: &CommandOutput) -> AppResult<u32> {
    if output.exit_status != 0 {
        return Err(AppError::ssh_command(
            output.exit_status,
            output.stderr.clone(),
        ));
    }
    output
        .stdout
        .trim()
        .parse::<u32>()
        .map_err(|_| AppError::Security("服务器返回了无效的账号权限信息".into()))
}
