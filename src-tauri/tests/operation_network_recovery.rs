use std::{collections::BTreeMap, sync::Arc};

use qingzhou_ssh_lib::{
    core::{
        secret_protector::SecretProtector,
        system_probe::{NetworkInterfaceCapability, SystemCapabilities},
        tasks::{built_in_catalog, plan_task, validate_parameters, ParameterKind},
    },
    domain::server::{CreateServerRequest, CredentialInput},
    error::AppResult,
    services::{
        app_services::AppServices,
        operation_restore_service::build_snapshot_rollback_command,
        operation_service::OperationPreflightRequest,
        remote_recovery_service::{
            build_ip_change_recovery_plan, build_remote_recovery_layout, parse_ip_recovery_state,
            validate_remote_recovery_observation, IpRecoveryState,
        },
    },
};
use serde_json::json;
use uuid::Uuid;

struct XorProtector;

impl SecretProtector for XorProtector {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0x4f).collect())
    }

    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0x4f).collect())
    }
}

fn capabilities(commands: &[&str]) -> SystemCapabilities {
    SystemCapabilities {
        os_id: "openeuler".into(),
        os_family: "openeuler".into(),
        version_id: Some("24.03".into()),
        package_manager: Some("dnf".into()),
        service_manager: "systemd".into(),
        architecture: "x86_64".into(),
        shell: "/bin/sh".into(),
        commands: commands.iter().map(|command| (*command).into()).collect(),
        services: Vec::new(),
        containers: Vec::new(),
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
fn hosts_and_firewall_parameters_are_owned_and_not_free_form() {
    let catalog = built_in_catalog()
        .into_iter()
        .map(|task| (task.id.clone(), task))
        .collect::<BTreeMap<_, _>>();
    let hosts = &catalog["network.hosts_manage"];
    let firewall = &catalog["security.firewall_open_port"];
    for task in [hosts, firewall] {
        assert!(task.parameters.iter().any(|parameter| {
            parameter.name == "entryId" && parameter.kind == ParameterKind::ManagedId
        }));
    }

    let id = "ea09032f-d4f8-4a7a-a850-abe6cbb88a40";
    assert!(validate_parameters(
        hosts,
        &json!({"action":"add", "entryId":id, "address":"10.0.0.8", "hostname":"app.internal"})
    )
    .is_ok());
    for invalid in [
        json!({"action":"add", "entryId":id, "address":"not-an-ip", "hostname":"app.internal"}),
        json!({"action":"add", "entryId":id, "address":"10.0.0.8", "hostname":"127.0.0.1"}),
        json!({"action":"flush", "entryId":id, "address":"10.0.0.8", "hostname":"app.internal"}),
        json!({"action":"add", "entryId":id, "address":"10.0.0.8", "hostname":"app.internal", "line":"10.0.0.8 app"}),
    ] {
        assert!(validate_parameters(hosts, &invalid).is_err(), "{invalid}");
    }

    assert!(validate_parameters(
        firewall,
        &json!({"action":"add", "entryId":id, "port":8080, "protocol":"tcp"})
    )
    .is_ok());
    for invalid in [
        json!({"action":"disable", "entryId":id, "port":8080, "protocol":"tcp"}),
        json!({"action":"flush", "entryId":id, "port":8080, "protocol":"tcp"}),
        json!({"action":"add", "entryId":id, "port":8080, "protocol":"all"}),
        json!({"action":"add", "entryId":id, "port":8080, "protocol":"tcp", "rule":"-j ACCEPT"}),
    ] {
        assert!(
            validate_parameters(firewall, &invalid).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn hosts_and_firewall_plans_use_concrete_single_rule_commands() {
    let catalog = built_in_catalog()
        .into_iter()
        .map(|task| (task.id.clone(), task))
        .collect::<BTreeMap<_, _>>();
    let id = "ea09032f-d4f8-4a7a-a850-abe6cbb88a40";
    let hosts = plan_task(
        &catalog["network.hosts_manage"],
        &capabilities(&[
            "getent", "sed", "awk", "mktemp", "stat", "chown", "chmod", "mv", "rm", "wc",
        ]),
        &json!({"action":"add", "entryId":id, "address":"10.0.0.8", "hostname":"app.internal"}),
    )
    .unwrap();
    assert!(hosts.execution_steps[0].command.contains("/etc/hosts"));
    assert!(hosts.execution_steps[0].command.contains("# qingzhou:"));

    for (backend, required) in [
        ("firewalld", vec!["firewall-cmd", "grep"]),
        ("ufw", vec!["ufw", "awk", "sed"]),
        ("nftables", vec!["nft", "awk"]),
        ("iptables", vec!["iptables"]),
    ] {
        let plan = plan_task(
            &catalog["security.firewall_open_port"],
            &capabilities(&required),
            &json!({"action":"add", "entryId":id, "port":8080, "protocol":"tcp"}),
        )
        .unwrap();
        assert_eq!(plan.implementation_id, backend);
        for step in plan.execution_steps.iter().chain(&plan.verify_steps) {
            assert!(!step.command.contains("{{"), "{backend}: {}", step.command);
            assert!(!step.command.contains("}}"), "{backend}: {}", step.command);
            for forbidden in [" flush", "--set-default", " disable", "-F "] {
                assert!(!step.command.contains(forbidden), "{backend}: {forbidden}");
            }
        }
    }
}

#[test]
fn firewall_snapshots_build_fixed_single_rule_rollbacks() {
    let id = "ea09032f-d4f8-4a7a-a850-abe6cbb88a40";
    for (backend, expected) in [
        ("firewalld", "firewall-cmd"),
        ("ufw", "ufw"),
        ("nftables", "nft"),
        ("iptables", "iptables"),
    ] {
        let snapshot = format!(
            "stdout:\nbackend={backend}\nentryid={id}\nport=8080\nprotocol=tcp\npresent=false\nstderr:\n"
        );
        let rollback =
            build_snapshot_rollback_command("security.firewall_open_port", &snapshot).unwrap();
        assert!(rollback.command.contains(expected));
        assert!(rollback.command.contains(id));
        assert!(!rollback.command.contains("flush"));
    }
    for snapshot in [
        "stdout:\nbackend=iptables\nentryid=bad;id\nport=8080\nprotocol=tcp\npresent=false\nstderr:\n",
        "stdout:\nbackend=iptables\nentryid=ea09032f-d4f8-4a7a-a850-abe6cbb88a40\nport=22;id\nprotocol=tcp\npresent=false\nstderr:\n",
        "stdout:\nbackend=iptables\nentryid=ea09032f-d4f8-4a7a-a850-abe6cbb88a40\nport=8080\nprotocol=all\npresent=false\nstderr:\n",
    ] {
        assert!(build_snapshot_rollback_command("security.firewall_open_port", snapshot).is_err());
    }
}

#[tokio::test]
async fn removing_the_current_ssh_port_is_rejected_before_preflight() {
    let root = tempfile::tempdir().unwrap();
    let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "SSH 端口保护".into(),
            host: "127.0.0.1".into(),
            port: 2222,
            username: "tester".into(),
            credential: CredentialInput::Password {
                password: "fixture".into(),
            },
        })
        .await
        .unwrap();
    let result = services
        .operation_service()
        .preflight_with_capabilities(
            &server.id,
            OperationPreflightRequest {
                task_id: "security.firewall_open_port".into(),
                task_version: 2,
                parameters: json!({
                    "action":"remove",
                    "entryId":"ea09032f-d4f8-4a7a-a850-abe6cbb88a40",
                    "port":2222,
                    "protocol":"tcp"
                }),
            },
            &capabilities(&["iptables"]),
        )
        .await;
    assert!(result.is_err());
}

#[test]
fn remote_recovery_layout_is_private_confined_and_checksum_bound() {
    let run_id = Uuid::nil();
    let sha256 = "a".repeat(64);
    let layout = build_remote_recovery_layout(run_id, &sha256).unwrap();
    assert!(layout.prepare_command.contains("${XDG_RUNTIME_DIR:-/tmp}"));
    assert!(layout.prepare_command.contains("mkdir -m 700"));
    assert!(layout.prepare_command.contains("umask 077"));
    assert!(layout.verify_command.contains(&sha256));
    assert!(!layout.prepare_command.contains(".."));

    let observation = format!(
        "path=/run/user/1000/qingzhou-recovery/{run_id}\nuid=1000\ndiruid=1000\ndirmode=700\nfilemode=600\nscriptmode=700\nsha256={sha256}\n"
    );
    validate_remote_recovery_observation(run_id, 1000, &sha256, &observation).unwrap();
    for invalid in [
        observation.replace("dirmode=700", "dirmode=777"),
        observation.replace("diruid=1000", "diruid=0"),
        observation.replace("qingzhou-recovery", "qingzhou-recovery/../escape"),
        observation.replace(&sha256, &"b".repeat(64)),
    ] {
        assert!(validate_remote_recovery_observation(run_id, 1000, &sha256, &invalid).is_err());
    }
}

#[test]
fn ip_change_requires_an_explicit_backend_and_rollback_scheduler() {
    let task = built_in_catalog()
        .into_iter()
        .find(|task| task.id == "network.ip_change")
        .unwrap();
    let parameters = json!({
        "interface":"eth0",
        "cidr":"10.20.30.40/24",
        "gateway":"10.20.30.1",
        "rollbackSeconds":120
    });
    for (commands, expected) in [
        (
            vec![
                "ip",
                "awk",
                "nmcli",
                "systemd-run",
                "systemctl",
                "base64",
                "sha256sum",
                "stat",
                "id",
                "mkdir",
                "chmod",
                "sed",
                "tr",
            ],
            "network-manager-systemd-run",
        ),
        (
            vec![
                "ip",
                "awk",
                "netplan",
                "at",
                "atq",
                "atrm",
                "base64",
                "sha256sum",
                "stat",
                "id",
                "mkdir",
                "chmod",
                "sed",
                "mv",
                "rm",
            ],
            "netplan-at",
        ),
    ] {
        let plan = plan_task(&task, &capabilities(&commands), &parameters).unwrap();
        assert_eq!(plan.implementation_id, expected);
        assert_eq!(plan.execution_steps.len(), 2);
        for step in plan.execution_steps.iter().chain(&plan.verify_steps) {
            assert!(!step.command.contains("{{"));
            assert!(!step.command.contains("managed:network"));
        }
    }

    assert!(plan_task(&task, &capabilities(&["ip", "awk", "nmcli"]), &parameters).is_err());
}

#[test]
fn ip_change_plan_arms_before_apply_and_commits_only_after_target_verification() {
    let run_id = Uuid::new_v4();
    let plan = build_ip_change_recovery_plan(
        run_id,
        "network-manager-systemd-run",
        "eth0",
        "10.20.30.40/24",
        "10.20.30.1",
        120,
    )
    .unwrap();
    assert!(plan.arm_command.contains("systemd-run"));
    assert!(plan.arm_command.contains("--on-active=120s"));
    assert!(plan.arm_command.contains("rollback_armed"));
    assert!(!plan.arm_command.contains("network_applied"));
    assert!(plan.apply_command.contains("network_applied"));
    assert!(!plan.apply_command.contains("rollback_cancelled"));
    assert!(plan.finalize_command.contains("target_connection_verified"));
    assert!(plan.finalize_command.contains("rollback_cancelled"));
    assert!(
        plan.finalize_command
            .find("target_connection_verified")
            .unwrap()
            < plan.finalize_command.find("rollback_cancelled").unwrap()
    );
    assert!(plan.rollback_script_sha256.len() == 64);
    assert!(!plan.arm_command.contains(".."));

    assert!(build_ip_change_recovery_plan(
        run_id,
        "network-manager-systemd-run",
        "eth0",
        "10.20.30.40/24",
        "2001:db8::1",
        120,
    )
    .is_err());
    assert!(build_ip_change_recovery_plan(
        run_id,
        "legacy-ifcfg-systemd-run",
        "eth0",
        "2001:db8::40/64",
        "2001:db8::1",
        120,
    )
    .is_err());
}

#[test]
fn ip_change_snapshot_builds_a_fixed_interface_rollback() {
    let snapshot = "stdout:\ninterface=eth0\naddresses=10.20.30.8/24,2001:db8::8/64\ngatewayfour=10.20.30.1\ngatewaysix=2001:db8::1\nstderr:\n";
    let rollback = build_snapshot_rollback_command("network.ip_change", snapshot).unwrap();
    assert!(rollback.command.contains("10.20.30.8/24"));
    assert!(rollback.command.contains("2001:db8::8/64"));
    assert!(rollback.command.contains("10.20.30.1"));
    assert!(!rollback.command.contains("{{"));

    for invalid in [
        snapshot.replace("interface=eth0", "interface=eth0;reboot"),
        snapshot.replace("10.20.30.8/24", "10.20.30.8/99"),
        snapshot.replace("gatewayfour=10.20.30.1", "gatewayfour=bad;gateway"),
    ] {
        assert!(build_snapshot_rollback_command("network.ip_change", &invalid).is_err());
    }

    let nm_snapshot = "stdout:\nbackend=networkmanager\ninterface=eth0\nconnectionb=V2lyZWQgMQ==\nipfourmethodb=bWFudWFs\nipfouraddressesb=MTAuMjAuMzAuOC8yNA==\nipfourgatewayb=MTAuMjAuMzAuMQ==\nipsixmethodb=aWdub3Jl\nipsixaddressesb=\nipsixgatewayb=\nstderr:\n";
    let nm_rollback = build_snapshot_rollback_command("network.ip_change", nm_snapshot).unwrap();
    assert!(nm_rollback.command.contains("nmcli connection modify"));
    assert!(nm_rollback.command.contains("Wired 1"));
    assert!(!nm_rollback.command.contains("V2lyZWQgMQ=="));
}

#[test]
fn ip_change_recovery_state_is_checksum_bound_and_one_way() {
    let sha256 = "a".repeat(64);
    assert_eq!(
        parse_ip_recovery_state(
            &format!("state=staged\nscriptsha={sha256}\ncommitted=false\n"),
            &sha256
        )
        .unwrap(),
        IpRecoveryState::Staged
    );
    assert_eq!(
        parse_ip_recovery_state(
            &format!("state=rolled_back\nscriptsha={sha256}\ncommitted=false\n"),
            &sha256
        )
        .unwrap(),
        IpRecoveryState::RolledBack
    );
    for invalid in [
        format!("state=committed\nscriptsha={sha256}\ncommitted=false\n"),
        format!(
            "state=rolled_back\nscriptsha={}\ncommitted=false\n",
            "b".repeat(64)
        ),
        format!("state=staged\nscriptsha={sha256}\ncommitted=false\nextra=value\n"),
    ] {
        assert!(parse_ip_recovery_state(&invalid, &sha256).is_err());
    }
}
