use std::time::Duration;

use qingzhou_ssh_lib::{
    core::{
        ssh::transport::{inspect_host_key, probe_system, SshEndpoint},
        system_probe::SystemCapabilities,
    },
    domain::server::StoredCredential,
    error::AppError,
};

fn endpoint() -> SshEndpoint {
    SshEndpoint {
        host: "127.0.0.1".into(),
        port: 2222,
        timeout: Duration::from_secs(10),
    }
}

fn assert_ubuntu(capabilities: &SystemCapabilities) {
    assert_eq!(capabilities.os_id, "ubuntu");
    assert_eq!(capabilities.os_family, "debian");
    assert_eq!(capabilities.package_manager.as_deref(), Some("apt"));
    assert_eq!(capabilities.service_manager, "systemd");
}

#[tokio::test]
#[ignore = "requires the project-local SSH fixture on 127.0.0.1:2222"]
async fn password_auth_and_probe_work_against_fixture() {
    let endpoint = endpoint();
    let observed = inspect_host_key(&endpoint).await.unwrap();
    let credential = StoredCredential::Password {
        password: "testpass".into(),
    };
    let capabilities = probe_system(
        &endpoint,
        "testuser",
        &credential,
        &observed.fingerprint_sha256,
    )
    .await
    .unwrap();
    assert_ubuntu(&capabilities);
}

#[tokio::test]
#[ignore = "requires the project-local SSH fixture and generated test key"]
async fn private_key_auth_and_probe_work_against_fixture() {
    let endpoint = endpoint();
    let observed = inspect_host_key(&endpoint).await.unwrap();
    let private_key_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.local/test-keys/id_ed25519");
    let credential = StoredCredential::PrivateKey {
        private_key: std::fs::read_to_string(private_key_path).unwrap(),
        passphrase: Some("fixture-passphrase".into()),
    };
    let capabilities = probe_system(
        &endpoint,
        "testuser",
        &credential,
        &observed.fingerprint_sha256,
    )
    .await
    .unwrap();
    assert_ubuntu(&capabilities);
}

#[tokio::test]
#[ignore = "requires the project-local SSH fixture on 127.0.0.1:2222"]
async fn wrong_fingerprint_blocks_before_authentication() {
    let endpoint = endpoint();
    let credential = StoredCredential::Password {
        password: "deliberately-wrong".into(),
    };
    let error = probe_system(
        &endpoint,
        "testuser",
        &credential,
        "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    )
    .await
    .unwrap_err();
    assert!(matches!(error, AppError::Security(_)));
}
