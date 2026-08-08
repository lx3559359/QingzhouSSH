use qingzhou_ssh_lib::core::{
    system_probe::{RemoteOsFamily, SystemCapabilities, POWERSHELL_PROBE_COMMANDS, PROBE_COMMAND},
    tasks::{
        built_in_catalog, evaluate_task_availability, remediation_for, TaskAvailabilityState,
        TaskDefinition,
    },
};

fn task(id: &str) -> TaskDefinition {
    built_in_catalog()
        .into_iter()
        .find(|definition| definition.id == id)
        .unwrap_or_else(|| panic!("missing task {id}"))
}

fn capabilities_for(definition: &TaskDefinition) -> SystemCapabilities {
    let mut commands = definition
        .implementations
        .iter()
        .flat_map(|implementation| implementation.compatibility.required_commands.clone())
        .collect::<Vec<_>>();
    commands.sort();
    commands.dedup();
    SystemCapabilities {
        os_id: "ubuntu".into(),
        os_family: "debian".into(),
        version_id: Some("24.04".into()),
        package_manager: Some("apt".into()),
        service_manager: "systemd".into(),
        architecture: "x86_64".into(),
        shell: "/bin/sh".into(),
        commands,
        services: Vec::new(),
        containers: Vec::new(),
        ..SystemCapabilities::default()
    }
}

#[test]
fn reports_ready_when_one_implementation_matches() {
    let definition = task("system.overview");
    let evaluation = evaluate_task_availability(&definition, &capabilities_for(&definition));

    assert_eq!(evaluation.state, TaskAvailabilityState::Ready);
    assert!(evaluation.implementation_id.is_some());
    assert!(evaluation.missing_commands.is_empty());
    assert!(evaluation.blocking_capabilities.is_empty());
}

#[test]
fn reports_missing_commands_instead_of_a_generic_compatibility_error() {
    let definition = task("network.packet_capture");
    let mut capabilities = capabilities_for(&definition);
    capabilities.commands.retain(|command| command != "tcpdump");

    let evaluation = evaluate_task_availability(&definition, &capabilities);

    assert_eq!(evaluation.state, TaskAvailabilityState::Remediable);
    assert_eq!(evaluation.missing_commands, ["tcpdump"]);
    assert!(evaluation.summary.contains("tcpdump"));
}

#[test]
fn reports_service_manager_mismatch_separately() {
    let definition = task("service.status");
    let mut capabilities = capabilities_for(&definition);
    capabilities.service_manager = "unknown".into();

    let evaluation = evaluate_task_availability(&definition, &capabilities);

    assert_eq!(evaluation.state, TaskAvailabilityState::Unsupported);
    assert!(evaluation
        .blocking_capabilities
        .iter()
        .any(|reason| reason.contains("服务管理器")));
}

#[test]
fn enables_ip_change_after_delayed_recovery_and_reconnect_verification_exist() {
    let definition = task("network.ip_change");
    let evaluation = evaluate_task_availability(&definition, &capabilities_for(&definition));

    assert_eq!(evaluation.state, TaskAvailabilityState::Ready);
    assert!(evaluation.implementation_id.is_some());
    assert!(evaluation.blocking_capabilities.is_empty());
}

#[test]
fn allows_generic_read_only_tools_on_unknown_distributions_when_commands_exist() {
    let definition = task("system.overview");
    let mut capabilities = capabilities_for(&definition);
    capabilities.os_id = "custom-linux".into();
    capabilities.os_family = "unknown".into();
    capabilities.platform_family = RemoteOsFamily::Linux;

    let evaluation = evaluate_task_availability(&definition, &capabilities);

    assert_eq!(evaluation.state, TaskAvailabilityState::Ready);
}

#[test]
fn maps_only_the_fixed_package_whitelist() {
    let apt = remediation_for(
        Some("apt"),
        &["tcpdump".into(), "dig".into(), "tcpdump".into()],
    )
    .expect("apt remediation");
    assert_eq!(apt.packages, ["dnsutils", "tcpdump"]);

    let dnf =
        remediation_for(Some("dnf"), &["ncat".into(), "iostat".into()]).expect("dnf remediation");
    assert_eq!(dnf.packages, ["nmap-ncat", "sysstat"]);

    let yum = remediation_for(Some("yum"), &["nc".into(), "lsof".into()]).expect("yum remediation");
    assert_eq!(yum.packages, ["lsof", "nmap-ncat"]);

    assert!(remediation_for(Some("apt"), &["made-up-command".into()]).is_none());
    assert!(remediation_for(Some("apk"), &["tcpdump".into()]).is_none());
}

#[test]
fn capability_probe_checks_every_command_required_by_the_builtin_catalog() {
    let scanned = PROBE_COMMAND
        .split("for cmd in ")
        .nth(1)
        .and_then(|tail| tail.split("; do").next())
        .expect("probe command allowlist");
    let mut scanned = scanned
        .split_whitespace()
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    scanned.extend(
        POWERSHELL_PROBE_COMMANDS
            .iter()
            .map(|command| (*command).to_owned()),
    );
    let required = built_in_catalog()
        .into_iter()
        .flat_map(|definition| definition.implementations)
        .flat_map(|implementation| implementation.compatibility.required_commands)
        .collect::<std::collections::BTreeSet<_>>();
    let missing = required
        .into_iter()
        .filter(|command| !scanned.contains(command.as_str()))
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "probe does not detect required commands: {missing:?}"
    );
}
