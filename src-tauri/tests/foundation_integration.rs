use std::sync::Arc;

use qingzhou_ssh_lib::{
    core::secret_protector::SecretProtector,
    domain::server::{CreateServerRequest, CredentialInput},
    error::AppResult,
    services::app_services::AppServices,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tempfile::TempDir;

struct XorProtector;

impl SecretProtector for XorProtector {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xA5).collect())
    }

    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xA5).collect())
    }
}

struct TestHarness {
    root: TempDir,
    services: AppServices,
}

impl TestHarness {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
            .await
            .unwrap();
        Self { root, services }
    }
}

#[tokio::test]
async fn creates_server_with_encrypted_credential_and_pending_host_trust() {
    let harness = TestHarness::new().await;
    let created = harness
        .services
        .create_server(CreateServerRequest {
            name: "网站服务器".into(),
            host: "127.0.0.1".into(),
            port: 2222,
            username: "testuser".into(),
            credential: CredentialInput::Password {
                password: "canary-password".into(),
            },
        })
        .await
        .unwrap();

    assert_eq!(harness.services.list_servers().await.unwrap().len(), 1);
    let vault_blob = std::fs::read(
        harness
            .root
            .path()
            .join(format!("vault/{}.bin", created.credential_id)),
    )
    .unwrap();
    assert!(!String::from_utf8_lossy(&vault_blob).contains("canary-password"));
    assert!(harness
        .services
        .get_trusted_host_key(&created.id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn removes_the_new_vault_entry_when_database_insert_fails() {
    let harness = TestHarness::new().await;
    let options = SqliteConnectOptions::new().filename(harness.root.path().join("app.db"));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TRIGGER reject_server_insert BEFORE INSERT ON servers BEGIN SELECT RAISE(FAIL, 'forced failure'); END",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let result = harness
        .services
        .create_server(CreateServerRequest {
            name: "失败服务器".into(),
            host: "127.0.0.1".into(),
            port: 22,
            username: "testuser".into(),
            credential: CredentialInput::Password {
                password: "must-not-remain".into(),
            },
        })
        .await;
    assert!(result.is_err());
    assert!(!std::fs::read_dir(harness.root.path().join("vault"))
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|value| value == "bin")));
}
