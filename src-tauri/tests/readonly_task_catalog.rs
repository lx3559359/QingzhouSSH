use std::collections::BTreeSet;

use qingzhou_ssh_lib::core::tasks::built_in_catalog;

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
