use qingzhou_ssh_lib::core::tasks::PrivilegeMode;
use qingzhou_ssh_lib::core::tasks::{fixed_install_command, PackageId, PackageManagerKind};
use qingzhou_ssh_lib::services::task_remediation_service::{
    ensure_same_binding, RemediationBinding, RemediationPreviewRegistry,
};
use uuid::Uuid;

#[test]
fn builds_only_fixed_noninteractive_install_commands() {
    let apt = fixed_install_command(
        PackageManagerKind::Apt,
        &[PackageId::Dnsutils, PackageId::Tcpdump],
    )
    .unwrap();
    assert_eq!(
        apt,
        "apt-get install -y --no-install-recommends dnsutils tcpdump"
    );

    let dnf = fixed_install_command(
        PackageManagerKind::Dnf,
        &[PackageId::NmapNcat, PackageId::Sysstat],
    )
    .unwrap();
    assert_eq!(dnf, "dnf install -y nmap-ncat sysstat");

    let yum = fixed_install_command(PackageManagerKind::Yum, &[PackageId::Lsof]).unwrap();
    assert_eq!(yum, "yum install -y lsof");

    for command in [apt, dnf, yum] {
        for forbidden in ["sudo -S", "SUDO_ASKPASS", "--stdin", ";", "&&", "|"] {
            assert!(
                !command.contains(forbidden),
                "{command} contains {forbidden}"
            );
        }
    }
}

#[test]
fn package_and_manager_enums_reject_arbitrary_input() {
    assert!(PackageManagerKind::try_from("apk").is_err());
    assert!(PackageId::try_from("curl; id").is_err());
    assert!(PackageId::try_from("../../tmp/package").is_err());
    assert!(fixed_install_command(PackageManagerKind::Apt, &[PackageId::NmapNcat]).is_err());
    assert!(fixed_install_command(PackageManagerKind::Dnf, &[PackageId::Dnsutils]).is_err());
}

fn binding() -> RemediationBinding {
    RemediationBinding {
        server_id: "server-1".into(),
        task_id: "network.packet_capture".into(),
        implementation_id: "tcpdump".into(),
        missing_commands: vec!["tcpdump".into()],
        packages: vec!["tcpdump".into()],
        package_manager: "apt".into(),
        privilege_mode: PrivilegeMode::PasswordlessSudo,
    }
}

#[tokio::test]
async fn confirmation_tokens_expire_and_are_consumed_once() {
    let registry = RemediationPreviewRegistry::default();
    let now = 1_000_000;
    let preview = registry.issue(binding(), now).await.unwrap();
    assert_eq!(preview.expires_at, now + 5 * 60 * 1000);

    assert!(registry
        .consume(preview.preview_id, Uuid::new_v4(), now + 1)
        .await
        .is_err());
    let consumed = registry
        .consume(preview.preview_id, preview.confirmation_token, now + 1)
        .await
        .unwrap();
    assert_eq!(consumed, binding());
    assert!(registry
        .consume(preview.preview_id, preview.confirmation_token, now + 2)
        .await
        .is_err());

    let expired = registry.issue(binding(), now).await.unwrap();
    assert!(registry
        .consume(
            expired.preview_id,
            expired.confirmation_token,
            expired.expires_at + 1
        )
        .await
        .is_err());
}

#[test]
fn every_security_binding_mismatch_is_rejected() {
    let expected = binding();
    let mutations = [
        RemediationBinding {
            server_id: "server-2".into(),
            ..expected.clone()
        },
        RemediationBinding {
            task_id: "network.udp".into(),
            ..expected.clone()
        },
        RemediationBinding {
            implementation_id: "other".into(),
            ..expected.clone()
        },
        RemediationBinding {
            missing_commands: vec!["lsof".into()],
            ..expected.clone()
        },
        RemediationBinding {
            packages: vec!["lsof".into()],
            ..expected.clone()
        },
        RemediationBinding {
            package_manager: "dnf".into(),
            ..expected.clone()
        },
    ];
    for changed in mutations {
        assert!(ensure_same_binding(&expected, &changed).is_err());
    }
    ensure_same_binding(&expected, &expected).unwrap();
}
