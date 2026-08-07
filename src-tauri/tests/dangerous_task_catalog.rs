use std::collections::BTreeMap;

use qingzhou_ssh_lib::core::system_probe::SystemCapabilities;
use qingzhou_ssh_lib::core::tasks::{
    built_in_catalog, plan_task, validate_parameters, BackupItemKind, ExecutionScope,
    ParameterKind, PrivilegeRequirement, RiskLevel,
};
use serde_json::json;

const REQUIRED_DANGEROUS_IDS: &[&str] = &[
    "system.hostname_change",
    "system.timezone_change",
    "system.time_sync_change",
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
        ("system.time_sync_change", BackupItemKind::RuntimeState),
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
    for invalid in [
        "/",
        "/etc/swapfile",
        "/tmp/swapfile",
        "/var/lib/qingzhou/swap/../escape",
    ] {
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
        "set-ntp",
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
                | "system.time_sync_change"
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

#[test]
fn system_storage_permission_plans_carry_every_recovery_phase() {
    let capabilities = SystemCapabilities {
        os_id: "openeuler".into(),
        os_family: "openeuler".into(),
        version_id: Some("24.03".into()),
        package_manager: Some("dnf".into()),
        service_manager: "systemd".into(),
        architecture: "x86_64".into(),
        shell: "/bin/sh".into(),
        commands: [
            "hostnamectl",
            "hostname",
            "timedatectl",
            "swapon",
            "swapoff",
            "mkswap",
            "stat",
            "chmod",
            "chown",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        services: Vec::new(),
        containers: Vec::new(),
        ..SystemCapabilities::default()
    };
    let cases = [
        ("system.hostname_change", json!({"hostname":"node-2"})),
        (
            "system.timezone_change",
            json!({"timezone":"Asia/Shanghai"}),
        ),
        ("system.time_sync_change", json!({"enabled":true})),
        (
            "storage.swap_manage",
            json!({"action":"create", "path":"/swapfile", "sizeMiB":1024}),
        ),
        (
            "security.file_permissions",
            json!({"path":"/etc/nginx/nginx.conf", "mode":"0644", "uid":0, "gid":0}),
        ),
    ];
    let catalog = built_in_catalog()
        .into_iter()
        .map(|task| (task.id.clone(), task))
        .collect::<BTreeMap<_, _>>();

    for (id, parameters) in cases {
        let plan = plan_task(&catalog[id], &capabilities, &parameters).unwrap();
        assert!(!plan.preview_steps.is_empty(), "{id}");
        assert!(plan.backup_plan.is_some(), "{id}");
        assert!(!plan.execution_steps.is_empty(), "{id}");
        assert!(!plan.verify_steps.is_empty(), "{id}");
        assert!(plan.rollback_plan.is_some(), "{id}");
    }
}

#[test]
fn service_cron_container_parameters_and_discovered_targets_are_strict() {
    let catalog = built_in_catalog()
        .into_iter()
        .map(|task| (task.id.clone(), task))
        .collect::<BTreeMap<_, _>>();

    for id in ["service.start", "service.stop", "service.restart"] {
        assert!(validate_parameters(&catalog[id], &json!({"service":"nginx;id"})).is_err());
    }
    assert!(validate_parameters(
        &catalog["container.action"],
        &json!({"container":"web$(id)", "action":"start"})
    )
    .is_err());

    let cron = &catalog["service.cron_manage"];
    assert!(cron.parameters.iter().any(|parameter| {
        parameter.name == "entryId" && parameter.kind == ParameterKind::ManagedId
    }));
    for invalid in [
        json!({"action":"add", "entryId":"not-a-uuid", "schedule":"0 2 * * *", "task":"system.overview"}),
        json!({"action":"add", "entryId":"00000000-0000-0000-0000-000000000000;id", "schedule":"0 2 * * *", "task":"system.overview"}),
        json!({"action":"add", "entryId":"9af25f52-72ab-4d53-b793-20f02f38d78a", "schedule":"61 2 * * *", "task":"system.overview"}),
        json!({"action":"add", "entryId":"9af25f52-72ab-4d53-b793-20f02f38d78a", "schedule":"0 2 * * *", "task":"service.status"}),
    ] {
        assert!(validate_parameters(cron, &invalid).is_err(), "{invalid}");
    }
}

#[test]
fn service_cron_container_plans_are_concrete_and_recoverable() {
    let capabilities = SystemCapabilities {
        os_id: "openeuler".into(),
        os_family: "openeuler".into(),
        version_id: Some("24.03".into()),
        package_manager: Some("dnf".into()),
        service_manager: "systemd".into(),
        architecture: "x86_64".into(),
        shell: "/bin/sh".into(),
        commands: [
            "systemctl",
            "awk",
            "sed",
            "mktemp",
            "wc",
            "chown",
            "chmod",
            "mv",
            "rm",
            "docker",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        services: vec!["nginx.service".into()],
        containers: vec!["web".into()],
        ..SystemCapabilities::default()
    };
    let entry_id = "9af25f52-72ab-4d53-b793-20f02f38d78a";
    let cases = [
        ("service.start", json!({"service":"nginx.service"})),
        (
            "service.boot_policy",
            json!({"service":"nginx.service", "policy":"enable"}),
        ),
        (
            "service.cron_manage",
            json!({"action":"add", "entryId":entry_id, "schedule":"0 2 * * *", "task":"system.overview"}),
        ),
        (
            "container.action",
            json!({"container":"web", "action":"pause"}),
        ),
    ];
    let catalog = built_in_catalog()
        .into_iter()
        .map(|task| (task.id.clone(), task))
        .collect::<BTreeMap<_, _>>();

    for (id, parameters) in cases {
        let plan = plan_task(&catalog[id], &capabilities, &parameters).unwrap();
        assert!(!plan.preview_steps.is_empty(), "{id}");
        assert!(plan.backup_plan.is_some(), "{id}");
        assert!(!plan.verify_steps.is_empty(), "{id}");
        assert!(plan.rollback_plan.is_some(), "{id}");
        for step in plan.execution_steps.iter().chain(&plan.verify_steps) {
            assert!(!step.command.contains("{{"), "{id}: {}", step.command);
            assert!(!step.command.contains("}}"), "{id}: {}", step.command);
        }
    }

    let cron = plan_task(
        &catalog["service.cron_manage"],
        &capabilities,
        &json!({"action":"remove", "entryId":entry_id, "schedule":"0 2 * * *", "task":"system.overview"}),
    )
    .unwrap();
    let command = &cron.execution_steps[0].command;
    assert!(command.contains("qz_marker=\"# qingzhou:$qz_id\""));
    assert!(command.contains(entry_id));
    assert!(command.contains("/etc/cron.d/qingzhou-managed"));
    assert!(!command.contains("rm -rf"));

    let mut missing_cron_tool = capabilities.clone();
    missing_cron_tool
        .commands
        .retain(|command| command != "mktemp");
    assert!(plan_task(
        &catalog["service.cron_manage"],
        &missing_cron_tool,
        &json!({"action":"add", "entryId":entry_id, "schedule":"0 2 * * *", "task":"system.overview"}),
    )
    .is_err());
}

#[test]
fn traditional_service_and_podman_are_selected_without_fallback_commands() {
    let capabilities = SystemCapabilities {
        os_id: "kylin".into(),
        os_family: "debian".into(),
        version_id: Some("10".into()),
        package_manager: Some("apt".into()),
        service_manager: "service".into(),
        architecture: "aarch64".into(),
        shell: "/bin/sh".into(),
        commands: ["service", "podman"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        services: vec!["nginx".into()],
        containers: vec!["web".into()],
        ..SystemCapabilities::default()
    };
    let catalog = built_in_catalog()
        .into_iter()
        .map(|task| (task.id.clone(), task))
        .collect::<BTreeMap<_, _>>();

    let service = plan_task(
        &catalog["service.start"],
        &capabilities,
        &json!({"service":"nginx"}),
    )
    .unwrap();
    assert_eq!(service.implementation_id, "service-start");
    assert!(service.execution_steps[0].command.starts_with("service "));

    let container = plan_task(
        &catalog["container.action"],
        &capabilities,
        &json!({"container":"web", "action":"start"}),
    )
    .unwrap();
    assert_eq!(container.implementation_id, "podman");
    assert!(container.execution_steps[0].command.starts_with("podman "));
}
