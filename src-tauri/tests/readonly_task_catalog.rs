use std::collections::{BTreeMap, BTreeSet};

use qingzhou_ssh_lib::core::{
    system_probe::{SystemCapabilities, PROBE_COMMAND},
    tasks::{
        built_in_catalog, plan_task, select_implementation, ExecutionScope, ParameterKind,
        RiskLevel, TaskCategory, TaskDefinition,
    },
};
use serde_json::json;

const REQUIRED_READONLY_IDS: &[&str] = &[
    "system.overview",
    "system.cpu_pressure",
    "system.memory_oom",
    "system.process_top",
    "system.process_query",
    "system.process_detail",
    "system.kernel_events",
    "system.boot_history",
    "system.time",
    "system.disk_usage",
    "storage.mounts_inode",
    "storage.io_latency",
    "storage.large_directories",
    "storage.deleted_open_files",
    "network.interface_health",
    "network.tcp_states",
    "network.listening_ports",
    "network.port_process",
    "network.ip_route",
    "network.dns",
    "network.connectivity",
    "network.http",
    "network.tls",
    "network.udp",
    "network.packet_capture",
    "security.ssh_events",
    "security.firewall_exposure",
    "service.inventory",
    "service.failed_logs",
    "service.status",
    "service.scheduled_tasks",
    "logs.search",
    "web.config_check",
    "container.health_storage",
    "container.inspect",
];

#[test]
fn readonly_catalog_contains_every_stable_id_once() {
    let catalog = built_in_catalog();
    for id in REQUIRED_READONLY_IDS {
        assert_eq!(
            catalog.iter().filter(|item| item.id == *id).count(),
            1,
            "{id}"
        );
    }

    let unique = catalog
        .iter()
        .map(|definition| definition.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(unique.len(), catalog.len(), "任务 ID 不允许重复");
}

fn by_id() -> BTreeMap<String, TaskDefinition> {
    built_in_catalog()
        .into_iter()
        .map(|definition| (definition.id.clone(), definition))
        .collect()
}

#[test]
fn system_and_storage_tasks_are_bounded_and_safe() {
    let catalog = by_id();
    assert_eq!(catalog["storage.large_directories"].parameters.len(), 3);
    assert!(
        catalog["storage.large_directories"].implementations[0].execution_steps[0].timeout_seconds
            <= 120
    );
    assert!(catalog["system.memory_oom"].implementations[0]
        .execution_steps
        .iter()
        .all(|step| step.output_limit_bytes <= 1024 * 1024));
    let pid = &catalog["system.process_detail"].parameters[0];
    assert!(pid.required);
    assert!(matches!(
        pid.kind,
        ParameterKind::Integer {
            min: 1,
            max: 4_194_304
        }
    ));
    assert!(catalog
        .values()
        .filter(|item| matches!(item.category, TaskCategory::System | TaskCategory::Storage))
        .all(|item| item.risk_level == RiskLevel::Safe));
    assert!(catalog
        .values()
        .filter(|item| matches!(item.category, TaskCategory::System | TaskCategory::Storage))
        .flat_map(|item| &item.implementations)
        .flat_map(|implementation| &implementation.execution_steps)
        .all(|step| !step.command_template.contains("find /")
            && !step.command_template.contains("du /")));
}

#[test]
fn active_network_tasks_have_explicit_limits() {
    let catalog = by_id();
    assert_eq!(
        catalog["network.packet_capture"].risk_level,
        RiskLevel::Caution
    );
    assert_eq!(
        catalog["network.packet_capture"].scope,
        ExecutionScope::SingleServer
    );
    assert!(catalog["network.udp"]
        .parameters
        .iter()
        .any(|parameter| parameter.name == "attempts"));
    assert!(catalog["network.connectivity"]
        .parameters
        .iter()
        .any(|parameter| parameter.name == "host"));
}

#[test]
fn packet_capture_builds_only_fixed_optional_filters() {
    let catalog = by_id();
    let capabilities = SystemCapabilities {
        os_id: "openeuler".into(),
        os_family: "openeuler".into(),
        version_id: Some("24.03".into()),
        package_manager: Some("dnf".into()),
        service_manager: "systemd".into(),
        architecture: "x86_64".into(),
        shell: "/bin/sh".into(),
        commands: ["tcpdump", "timeout", "wc"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        services: Vec::new(),
        containers: Vec::new(),
    };
    let plan = plan_task(
        &catalog["network.packet_capture"],
        &capabilities,
        &json!({"interface":"eth0","count":20,"seconds":5}),
    )
    .unwrap();
    let command = &plan.execution_steps[0].command;
    assert!(!command.contains("{{"));
    assert!(command.contains("qz_host=''") && command.contains("qz_port=''"));
    assert!(!catalog["network.packet_capture"]
        .parameters
        .iter()
        .any(|parameter| parameter.name == "filter"));
}

#[test]
fn capability_probe_covers_new_diagnostic_implementations() {
    for command in [
        "ss",
        "netstat",
        "getent",
        "ping",
        "curl",
        "openssl",
        "timeout",
        "nc",
        "ncat",
        "tcpdump",
        "wc",
        "du",
        "sort",
        "lsof",
        "iostat",
        "findmnt",
        "journalctl",
        "sshd",
        "docker",
        "podman",
        "nginx",
        "apachectl",
    ] {
        assert!(
            PROBE_COMMAND.contains(&format!(" {command} ")),
            "能力探针缺少 {command}"
        );
    }
}

#[test]
fn service_web_and_container_targets_follow_discovery() {
    let catalog = by_id();
    let systemd = detected_capabilities("systemd", &["systemctl", "head"], &["nginx.service"], &[]);
    assert_eq!(
        select_implementation(&catalog["service.inventory"], &systemd)
            .unwrap()
            .id,
        "systemd"
    );
    assert!(plan_task(
        &catalog["service.status"],
        &systemd,
        &json!({"service":"nginx.service"})
    )
    .is_ok());
    assert!(plan_task(
        &catalog["service.status"],
        &systemd,
        &json!({"service":"missing.service"})
    )
    .is_err());

    let nginx = detected_capabilities("systemd", &["nginx", "ss", "head"], &[], &[]);
    assert_eq!(
        select_implementation(&catalog["web.config_check"], &nginx)
            .unwrap()
            .id,
        "nginx"
    );
    let apache = detected_capabilities("systemd", &["apachectl", "ss", "head"], &[], &[]);
    assert_eq!(
        select_implementation(&catalog["web.config_check"], &apache)
            .unwrap()
            .id,
        "apache"
    );

    let docker = detected_capabilities("systemd", &["docker", "head"], &[], &["web"]);
    assert_eq!(
        select_implementation(&catalog["container.health_storage"], &docker)
            .unwrap()
            .id,
        "docker"
    );
    assert!(plan_task(
        &catalog["container.inspect"],
        &docker,
        &json!({"container":"web","action":"logs","lines":100})
    )
    .is_ok());
    assert!(plan_task(
        &catalog["container.inspect"],
        &docker,
        &json!({"container":"database","action":"inspect","lines":100})
    )
    .is_err());
}

fn detected_capabilities(
    service_manager: &str,
    commands: &[&str],
    services: &[&str],
    containers: &[&str],
) -> SystemCapabilities {
    SystemCapabilities {
        os_id: "openeuler".into(),
        os_family: "openeuler".into(),
        version_id: Some("24.03".into()),
        package_manager: Some("dnf".into()),
        service_manager: service_manager.into(),
        architecture: "x86_64".into(),
        shell: "/bin/sh".into(),
        commands: commands.iter().map(|value| (*value).into()).collect(),
        services: services.iter().map(|value| (*value).into()).collect(),
        containers: containers.iter().map(|value| (*value).into()).collect(),
    }
}
