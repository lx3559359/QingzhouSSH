use qingzhou_ssh_lib::core::{
    system_probe::{NetworkInterfaceCapability, SystemCapabilities},
    tasks::{built_in_catalog, plan_task, validate_scope},
};
use serde_json::json;

fn capabilities(os_id: &str, family: &str, service: &str, commands: &[&str]) -> SystemCapabilities {
    SystemCapabilities {
        os_id: os_id.into(),
        os_family: family.into(),
        version_id: Some("1".into()),
        package_manager: None,
        service_manager: service.into(),
        architecture: "x86_64".into(),
        shell: "/bin/sh".into(),
        commands: commands.iter().map(|value| (*value).into()).collect(),
        services: vec!["nginx".into(), "nginx.service".into()],
        containers: vec!["web".into()],
        interfaces: vec![NetworkInterfaceCapability {
            name: "eth0".into(),
            is_up: true,
            is_default: true,
            addresses: vec!["192.0.2.10/24".into()],
            gateway4: Some("192.0.2.1".into()),
            gateway6: None,
        }],
        ..SystemCapabilities::default()
    }
}

#[test]
fn planner_selects_capability_and_never_exposes_commands() {
    let definition = built_in_catalog()
        .into_iter()
        .find(|item| item.id == "service.status")
        .unwrap();
    let plan = plan_task(
        &definition,
        &capabilities("openeuler", "openeuler", "systemd", &["systemctl"]),
        &json!({"service":"nginx.service"}),
    )
    .unwrap();
    assert_eq!(plan.implementation_id, "systemd-status");
    assert_eq!(plan.execution_steps.len(), 1);
    assert_eq!(plan.execution_steps[0].title, "采集服务诊断");
    assert!(plan.execution_steps[0]
        .command
        .contains("systemctl show --no-pager"));
    assert!(plan.execution_steps[0]
        .command
        .contains("systemctl status --no-pager --lines=100 -- 'nginx.service'"));
    let public_json = serde_json::to_string(&plan.public_summary()).unwrap();
    for private_value in ["systemctl", "nginx.service", "command"] {
        assert!(!public_json.contains(private_value));
    }
}

#[test]
fn planner_rejects_batch_for_non_safe_tasks() {
    let catalog = built_in_catalog();
    let dangerous = catalog
        .iter()
        .find(|item| item.id == "service.restart")
        .unwrap();
    assert!(validate_scope(dangerous, 2).is_err());
    assert!(validate_scope(dangerous, 1).is_ok());

    let safe = catalog
        .iter()
        .find(|item| item.id == "system.disk_usage")
        .unwrap();
    assert!(validate_scope(safe, 2).is_ok());
    assert!(validate_scope(safe, 0).is_err());
}

#[test]
fn planner_requires_ip_change_interface_to_come_from_server_discovery() {
    let definition = built_in_catalog()
        .into_iter()
        .find(|item| item.id == "network.ip_change")
        .unwrap();
    let mut detected = capabilities("ubuntu", "debian", "systemd", &[]);
    detected.commands = definition.implementations[0]
        .compatibility
        .required_commands
        .clone();

    let error = plan_task(
        &definition,
        &detected,
        &json!({
            "interface": "eth9",
            "cidr": "192.0.2.20/24",
            "gateway": "192.0.2.1",
            "rollbackSeconds": 120
        }),
    )
    .unwrap_err();

    assert_eq!(error.code(), "compatibility");
}
