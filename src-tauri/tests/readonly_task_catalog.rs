use std::collections::{BTreeMap, BTreeSet};

use qingzhou_ssh_lib::core::tasks::{
    built_in_catalog, ParameterKind, RiskLevel, TaskCategory, TaskDefinition,
};

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
