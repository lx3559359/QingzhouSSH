use qingzhou_ssh_lib::{
    core::{
        ssh::transport::CommandOutput,
        tasks::{
            built_in_catalog, elevate_fixed_command, evaluate_privilege_probe, BackupItemKind,
            PrivilegeMode, PrivilegeRequirement, RiskLevel, PASSWORDLESS_SUDO_PROBE_COMMAND,
            PRIVILEGE_UID_COMMAND,
        },
    },
    domain::operation::OperationStatus,
};

fn output(stdout: &str, stderr: &str, exit_status: i32) -> CommandOutput {
    CommandOutput {
        stdout: stdout.into(),
        stderr: stderr.into(),
        exit_status,
    }
}

#[test]
fn dangerous_task_accepts_only_root_or_passwordless_sudo() {
    assert_eq!(
        evaluate_privilege_probe(&output("0\n", "", 0), None).unwrap(),
        PrivilegeMode::Root
    );
    assert_eq!(
        evaluate_privilege_probe(&output("1000\n", "", 0), Some(&output("", "", 0))).unwrap(),
        PrivilegeMode::PasswordlessSudo
    );

    let error = evaluate_privilege_probe(
        &output("1000\n", "", 0),
        Some(&output("", "sudo: a password is required", 1)),
    )
    .unwrap_err();
    assert_eq!(error.code(), "passwordless_sudo_required");
    assert!(error.to_string().contains("服务器未发生修改"));
    assert!(!error.to_string().contains("sudo -S"));
}

#[test]
fn privilege_probe_and_elevation_use_only_noninteractive_fixed_commands() {
    assert_eq!(PRIVILEGE_UID_COMMAND, "id -u");
    assert_eq!(PASSWORDLESS_SUDO_PROBE_COMMAND, "sudo -n true");

    let command = "systemctl restart -- 'nginx.service'";
    assert_eq!(
        elevate_fixed_command(command, PrivilegeMode::Root).unwrap(),
        command
    );
    let elevated = elevate_fixed_command(command, PrivilegeMode::PasswordlessSudo).unwrap();
    assert!(elevated.starts_with("sudo -n -- sh -c "));
    for forbidden in ["sudo -S", "SUDO_ASKPASS", "--stdin"] {
        assert!(!elevated.contains(forbidden));
        assert!(!PRIVILEGE_UID_COMMAND.contains(forbidden));
        assert!(!PASSWORDLESS_SUDO_PROBE_COMMAND.contains(forbidden));
    }
}

#[test]
fn malformed_uid_output_is_rejected_without_trying_sudo() {
    let error = evaluate_privilege_probe(&output("root\n", "", 0), None).unwrap_err();
    assert_eq!(error.code(), "security");
}

#[test]
fn dangerous_lifecycle_rejects_running_before_backup() {
    assert!(OperationStatus::WaitingConfirmation.can_transition_to(OperationStatus::BackingUp));
    assert!(OperationStatus::BackingUp.can_transition_to(OperationStatus::Running));
    assert!(!OperationStatus::WaitingConfirmation.can_transition_to(OperationStatus::Running));
}

#[test]
fn catalog_has_readonly_previews_and_safe_recovery_metadata() {
    for definition in built_in_catalog() {
        if definition.risk_level == RiskLevel::Dangerous {
            assert_eq!(
                definition.privilege,
                PrivilegeRequirement::RootOrPasswordlessSudo,
                "{} must require an elevated preflight",
                definition.id
            );
        }
        for implementation in &definition.implementations {
            assert!(
                !implementation.preview_steps.is_empty(),
                "{} / {} must expose a no-side-effect preview",
                definition.id,
                implementation.id
            );
            if definition.risk_level != RiskLevel::Dangerous {
                assert!(implementation.backup_plan.is_none());
                assert!(implementation.rollback_plan.is_none());
            }
        }
    }

    let allowed = [
        BackupItemKind::RemoteFile,
        BackupItemKind::CommandSnapshot,
        BackupItemKind::ManagedBlock,
        BackupItemKind::RuntimeState,
    ];
    assert_eq!(allowed.len(), 4);
}
