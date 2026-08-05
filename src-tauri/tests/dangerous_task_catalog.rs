use std::collections::BTreeMap;

use qingzhou_ssh_lib::core::tasks::{
    built_in_catalog, validate_parameters, BackupItemKind, ExecutionScope, PrivilegeRequirement,
    RiskLevel,
};
use serde_json::json;

const REQUIRED_DANGEROUS_IDS: &[&str] = &[
    "system.hostname_change",
    "system.timezone_change",
    "storage.swap_manage",
    "security.file_permissions",
    "network.hosts_manage",
    "network.ip_change",
    "security.firewall_open_port",
    "service.start",
    "service.stop",
    "service.restart",
    "service.boot_policy",
    "service.cron_manage",
    "container.action",
];

#[test]
fn every_builtin_dangerous_task_has_a_complete_recovery_contract() {
    let catalog = built_in_catalog()
        .into_iter()
        .map(|task| (task.id.clone(), task))
        .collect::<BTreeMap<_, _>>();
    for id in REQUIRED_DANGEROUS_IDS {
        let task = catalog.get(*id).unwrap_or_else(|| panic!("missing {id}"));
        assert_eq!(task.risk_level, RiskLevel::Dangerous, "{id}");
        assert_eq!(task.scope, ExecutionScope::SingleServer, "{id}");
        assert_eq!(
            task.privilege,
            PrivilegeRequirement::RootOrPasswordlessSudo,
            "{id}"
        );
        assert!(!task.parameters.is_empty(), "{id}");
        for implementation in &task.implementations {
            assert!(!implementation.preview_steps.is_empty(), "{id}");
            assert!(implementation.backup_plan.is_some(), "{id}");
            assert!(!implementation.execution_steps.is_empty(), "{id}");
            assert!(!implementation.verify_steps.is_empty(), "{id}");
            assert!(implementation.rollback_plan.is_some(), "{id}");
            assert!(
                !implementation
                    .backup_plan
                    .as_ref()
                    .unwrap()
                    .items
                    .is_empty(),
                "{id}"
            );
            assert!(
                !implementation
                    .rollback_plan
                    .as_ref()
                    .unwrap()
                    .steps
                    .is_empty(),
                "{id}"
            );
        }
        let encoded = serde_json::to_string(task).unwrap();
        assert!(!encoded.contains("commandTemplate"), "{id}");
        assert!(!encoded.contains("targetTemplate"), "{id}");
    }
}

#[test]
fn dangerous_recovery_matrix_uses_the_expected_backup_kinds() {
    let catalog = built_in_catalog()
        .into_iter()
        .map(|task| (task.id.clone(), task))
        .collect::<BTreeMap<_, _>>();
    let expected = [
        ("system.hostname_change", BackupItemKind::RuntimeState),
        ("system.timezone_change", BackupItemKind::RuntimeState),
        ("storage.swap_manage", BackupItemKind::CommandSnapshot),
        ("security.file_permissions", BackupItemKind::RuntimeState),
        ("network.hosts_manage", BackupItemKind::RemoteFile),
        ("network.ip_change", BackupItemKind::CommandSnapshot),
        (
            "security.firewall_open_port",
            BackupItemKind::CommandSnapshot,
        ),
        ("service.start", BackupItemKind::RuntimeState),
        ("service.stop", BackupItemKind::RuntimeState),
        ("service.restart", BackupItemKind::RuntimeState),
        ("service.boot_policy", BackupItemKind::RuntimeState),
        ("service.cron_manage", BackupItemKind::ManagedBlock),
        ("container.action", BackupItemKind::RuntimeState),
    ];
    for (id, expected_kind) in expected {
        let task = catalog.get(id).unwrap();
        assert!(task.implementations.iter().all(|implementation| {
            implementation
                .backup_plan
                .as_ref()
                .unwrap()
                .items
                .iter()
                .any(|item| item.kind == expected_kind)
        }));
    }
}

#[test]
fn previews_are_distinct_from_mutating_and_rollback_commands() {
    for task in built_in_catalog()
        .into_iter()
        .filter(|task| REQUIRED_DANGEROUS_IDS.contains(&task.id.as_str()))
    {
        for implementation in task.implementations {
            let preview = implementation
                .preview_steps
                .iter()
                .map(|step| step.command_template.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let execution = implementation
                .execution_steps
                .iter()
                .map(|step| step.command_template.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let rollback = implementation
                .rollback_plan
                .as_ref()
                .unwrap()
                .steps
                .iter()
                .map(|step| step.command_template.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            assert_ne!(preview, execution, "{}", task.id);
            assert_ne!(preview, rollback, "{}", task.id);
        }
    }
}

#[test]
fn system_storage_permissions_reject_unsafe_targets_before_execution() {
    let catalog = built_in_catalog()
        .into_iter()
        .map(|task| (task.id.clone(), task))
        .collect::<BTreeMap<_, _>>();

    let hostname = &catalog["system.hostname_change"];
    for invalid in ["bad name", "-leading.example", "name;id"] {
        assert!(validate_parameters(hostname, &json!({"hostname": invalid})).is_err());
    }

    let timezone = &catalog["system.timezone_change"];
    for invalid in ["../UTC", "Asia/Shanghai;id", "/etc/localtime", "A B"] {
        assert!(validate_parameters(timezone, &json!({"timezone": invalid})).is_err());
    }
    assert!(validate_parameters(timezone, &json!({"timezone": "Asia/Shanghai"})).is_ok());

    let swap = &catalog["storage.swap_manage"];
    for invalid in ["/", "/etc/swapfile", "/tmp/swapfile", "/var/lib/qingzhou/swap/../escape"] {
        assert!(validate_parameters(
            swap,
            &json!({"action":"create", "path":invalid, "sizeMiB":1024})
        )
        .is_err());
    }
    assert!(validate_parameters(
        swap,
        &json!({"action":"create", "path":"/swapfile", "sizeMiB":64})
    )
    .is_ok());

    let permissions = &catalog["security.file_permissions"];
    for invalid in ["/", "/etc", "/usr", "/var", "/home"] {
        assert!(validate_parameters(
            permissions,
            &json!({"path":invalid, "mode":"0644", "uid":0, "gid":0})
        )
        .is_err());
    }
    assert!(validate_parameters(
        permissions,
        &json!({"path":"/etc/nginx/nginx.conf", "mode":"0644", "uid":0, "gid":0})
    )
    .is_ok());
}

#[test]
fn system_storage_permission_previews_are_read_only() {
    let forbidden = [
        "set-hostname",
        "set-timezone",
        "fallocate",
        "mkswap",
        "swapoff",
        " chown ",
        " chmod ",
        " rm ",
    ];
    for task in built_in_catalog().into_iter().filter(|task| {
        matches!(
            task.id.as_str(),
            "system.hostname_change"
                | "system.timezone_change"
                | "storage.swap_manage"
                | "security.file_permissions"
        )
    }) {
        for implementation in task.implementations {
            let preview = implementation
                .preview_steps
                .iter()
                .map(|step| step.command_template.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            for fragment in forbidden {
                assert!(!preview.contains(fragment), "{}: {fragment}", task.id);
            }
        }
    }
}
