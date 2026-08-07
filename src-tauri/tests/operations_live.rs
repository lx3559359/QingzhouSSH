use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use qingzhou_ssh_lib::{
    core::{secret_protector::SecretProtector, ssh::executor::VecEventSink},
    domain::{
        operation::{OperationPhase, OperationStatus, OperationStepStatus},
        operation_restore::OperationRestorePointStatus,
        server::{CreateServerRequest, CredentialInput},
    },
    error::AppResult,
    services::{
        app_services::AppServices,
        operation_service::{OperationPreflightRequest, OperationStartRequest},
    },
};
use serde_json::json;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

struct XorProtector;

impl SecretProtector for XorProtector {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0x6d).collect())
    }

    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0x6d).collect())
    }
}

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        if self.0.starts_with(project_root().join(".local/test-data")) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap()
}

fn fixture_remote_root() -> PathBuf {
    project_root().join(".local/ssh-fixture/remote-root")
}

fn fixture_state(name: &str) -> String {
    std::fs::read_to_string(
        fixture_remote_root()
            .join("run/qingzhou-fixture")
            .join(format!("{name}.state")),
    )
    .unwrap()
    .trim()
    .into()
}

async fn trusted_fixture_as(services: &AppServices, username: &str) -> String {
    let server = services
        .create_server(CreateServerRequest {
            name: "危险任务恢复夹具".into(),
            host: "127.0.0.1".into(),
            port: 2222,
            username: username.into(),
            credential: CredentialInput::Password {
                password: "testpass".into(),
            },
        })
        .await
        .unwrap();
    let check = services.inspect_host_key(&server.id).await.unwrap();
    services
        .trust_host_key(&server.id, check.observed)
        .await
        .unwrap();
    server.id
}

async fn trusted_fixture(services: &AppServices) -> String {
    trusted_fixture_as(services, "testuser").await
}

async fn run_hostname(
    services: &AppServices,
    server_id: &str,
    hostname: &str,
) -> qingzhou_ssh_lib::domain::operation::OperationDetails {
    let operations = services.operation_service();
    let request = OperationPreflightRequest {
        task_id: "system.hostname_change".into(),
        task_version: 2,
        parameters: json!({"hostname":hostname}),
    };
    let preview = operations
        .preflight(server_id, request.clone())
        .await
        .unwrap();
    let mut events = VecEventSink::default();
    operations
        .start(
            server_id,
            OperationStartRequest {
                task_id: request.task_id,
                task_version: request.task_version,
                parameters: request.parameters,
                confirmed_preview_id: Some(preview.preview_id),
            },
            &mut events,
        )
        .await
        .unwrap()
}

async fn run_dangerous_task(
    services: &AppServices,
    server_id: &str,
    task_id: &str,
    parameters: Value,
) -> qingzhou_ssh_lib::domain::operation::OperationDetails {
    let operations = services.operation_service();
    let request = OperationPreflightRequest {
        task_id: task_id.into(),
        task_version: 2,
        parameters,
    };
    let preview = operations
        .preflight(server_id, request.clone())
        .await
        .unwrap();
    let mut events = VecEventSink::default();
    operations
        .start(
            server_id,
            OperationStartRequest {
                task_id: request.task_id,
                task_version: request.task_version,
                parameters: request.parameters,
                confirmed_preview_id: Some(preview.preview_id),
            },
            &mut events,
        )
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires scripts/ssh-fixture.ps1 -Action Start"]
async fn dangerous_hostname_success_and_verify_failure_close_the_recovery_loop() {
    let data_root = project_root()
        .join(".local/test-data")
        .join(format!("operations-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&data_root).unwrap();
    let _cleanup = Cleanup(data_root.clone());
    let services = AppServices::open_with_protector(&data_root, Arc::new(XorProtector))
        .await
        .unwrap();
    let server_id = trusted_fixture(&services).await;

    let succeeded = run_hostname(&services, &server_id, "fixture-success").await;
    assert_eq!(succeeded.run.status, OperationStatus::Succeeded);
    for phase in [
        OperationPhase::Backup,
        OperationPhase::Execute,
        OperationPhase::Verify,
    ] {
        assert!(succeeded
            .steps
            .iter()
            .any(|step| { step.phase == phase && step.status == OperationStepStatus::Succeeded }));
    }
    let successful_restore = services
        .operation_restore_service()
        .list_by_run(succeeded.run.id)
        .await
        .unwrap();
    assert_eq!(successful_restore.len(), 1);
    assert_eq!(
        successful_restore[0].point.status,
        OperationRestorePointStatus::Available
    );

    let rolled_back = run_hostname(&services, &server_id, "fixture-verify-failure").await;
    assert_eq!(rolled_back.run.status, OperationStatus::RolledBack);
    assert!(rolled_back.steps.iter().any(|step| {
        step.phase == OperationPhase::Rollback && step.status == OperationStepStatus::Succeeded
    }));
    let failed_restore = services
        .operation_restore_service()
        .list_by_run(rolled_back.run.id)
        .await
        .unwrap();
    assert_eq!(failed_restore.len(), 1);
    assert_eq!(
        failed_restore[0].point.status,
        OperationRestorePointStatus::RolledBack
    );
    assert!(data_root.join("backups/tasks").is_dir());
}

#[tokio::test]
#[ignore = "requires scripts/ssh-fixture.ps1 -Action Start"]
async fn timezone_and_time_sync_changes_use_detected_current_state() {
    let data_root = project_root()
        .join(".local/test-data")
        .join(format!("operations-time-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&data_root).unwrap();
    let _cleanup = Cleanup(data_root.clone());
    let services = AppServices::open_with_protector(&data_root, Arc::new(XorProtector))
        .await
        .unwrap();
    let server_id = trusted_fixture(&services).await;

    let timezone = run_dangerous_task(
        &services,
        &server_id,
        "system.timezone_change",
        json!({"timezone":"Asia/Shanghai"}),
    )
    .await;
    assert_eq!(
        timezone.run.status,
        OperationStatus::Succeeded,
        "{timezone:#?}"
    );
    assert_eq!(fixture_state("timezone"), "Asia/Shanghai");

    for enabled in [false, true] {
        let time_sync = run_dangerous_task(
            &services,
            &server_id,
            "system.time_sync_change",
            json!({"enabled":enabled}),
        )
        .await;
        assert_eq!(
            time_sync.run.status,
            OperationStatus::Succeeded,
            "{time_sync:#?}"
        );
        assert_eq!(fixture_state("ntp"), enabled.to_string());
    }

    assert!(data_root.join("backups/tasks").is_dir());
}

#[tokio::test]
#[ignore = "requires scripts/ssh-fixture.ps1 -Action Start"]
async fn service_and_container_actions_use_stateful_backup_verify_and_rollback() {
    let data_root = project_root()
        .join(".local/test-data")
        .join(format!("operations-task6-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&data_root).unwrap();
    let _cleanup = Cleanup(data_root.clone());
    let services = AppServices::open_with_protector(&data_root, Arc::new(XorProtector))
        .await
        .unwrap();
    let server_id = trusted_fixture(&services).await;

    let service = run_dangerous_task(
        &services,
        &server_id,
        "service.stop",
        json!({"service":"qingzhou-fixture.service"}),
    )
    .await;
    assert_eq!(
        service.run.status,
        OperationStatus::Succeeded,
        "{service:#?}"
    );

    let rolled_back = run_dangerous_task(
        &services,
        &server_id,
        "service.stop",
        json!({"service":"qingzhou-verify-fail.service"}),
    )
    .await;
    assert_eq!(rolled_back.run.status, OperationStatus::RolledBack);
    assert!(rolled_back.steps.iter().any(|step| {
        step.phase == OperationPhase::Rollback && step.status == OperationStepStatus::Succeeded
    }));

    let container = run_dangerous_task(
        &services,
        &server_id,
        "container.action",
        json!({"container":"fixture-container", "action":"pause"}),
    )
    .await;
    assert_eq!(container.run.status, OperationStatus::Succeeded);
    assert!(data_root.join("backups/tasks").is_dir());
}

#[tokio::test]
#[ignore = "requires scripts/ssh-fixture.ps1 -Action Start"]
async fn preview_is_read_only_and_hashed_restore_assets_can_be_rolled_back_and_cleaned() {
    let data_root = project_root()
        .join(".local/test-data")
        .join(format!("operations-preview-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&data_root).unwrap();
    let _cleanup = Cleanup(data_root.clone());
    let services = AppServices::open_with_protector(&data_root, Arc::new(XorProtector))
        .await
        .unwrap();
    let server_id = trusted_fixture(&services).await;
    let operations = services.operation_service();
    let request = OperationPreflightRequest {
        task_id: "system.hostname_change".into(),
        task_version: 2,
        parameters: json!({"hostname":"fixture-hashed-backup"}),
    };

    let hostname_before = fixture_state("hostname");
    let preview = operations
        .preflight(&server_id, request.clone())
        .await
        .unwrap();
    assert_eq!(fixture_state("hostname"), hostname_before);
    assert_eq!(preview.confirmation_token, Some(preview.preview_id));
    assert!(!preview.current_state_summary.trim().is_empty());
    assert!(!data_root
        .join("backups/tasks")
        .join(preview.preview_id.to_string())
        .exists());

    let mut events = VecEventSink::default();
    let succeeded = operations
        .start(
            &server_id,
            OperationStartRequest {
                task_id: request.task_id,
                task_version: request.task_version,
                parameters: request.parameters,
                confirmed_preview_id: preview.confirmation_token,
            },
            &mut events,
        )
        .await
        .unwrap();
    assert_eq!(succeeded.run.status, OperationStatus::Succeeded);

    let restore = services
        .operation_restore_service()
        .list_by_run(succeeded.run.id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(restore.point.status, OperationRestorePointStatus::Available);
    assert!(!restore.items.is_empty());
    for item in &restore.items {
        let relative = item.local_relative_path.as_ref().unwrap();
        let local_path = data_root.join(relative);
        assert!(local_path.starts_with(&data_root));
        assert!(local_path.is_file());
        let actual_sha = format!("{:x}", Sha256::digest(std::fs::read(&local_path).unwrap()));
        assert_eq!(item.sha256.as_deref(), Some(actual_sha.as_str()));
    }

    let recovery = operations
        .rollback_operation(restore.point.id)
        .await
        .unwrap();
    assert_eq!(recovery.operation.run.status, OperationStatus::RolledBack);
    assert_eq!(fixture_state("hostname"), hostname_before);
    let cleaned = services
        .operation_restore_service()
        .cleanup_assets(restore.point.id)
        .await
        .unwrap();
    assert_eq!(cleaned.point.status, OperationRestorePointStatus::Expired);
    assert!(!data_root
        .join("backups/tasks")
        .join(succeeded.run.id.to_string())
        .exists());
}

#[tokio::test]
#[ignore = "requires scripts/ssh-fixture.ps1 -Action Start"]
async fn dangerous_preflight_supports_root_and_passwordless_sudo_but_rejects_no_privilege() {
    let data_root = project_root().join(".local/test-data").join(format!(
        "operations-privilege-live-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_root).unwrap();
    let _cleanup = Cleanup(data_root.clone());
    let services = AppServices::open_with_protector(&data_root, Arc::new(XorProtector))
        .await
        .unwrap();
    let request = OperationPreflightRequest {
        task_id: "system.hostname_change".into(),
        task_version: 2,
        parameters: json!({"hostname":"fixture-privilege-preview"}),
    };

    for username in ["root-sim", "sudo-user"] {
        let server_id = trusted_fixture_as(&services, username).await;
        let preview = services
            .operation_service()
            .preflight(&server_id, request.clone())
            .await
            .unwrap();
        assert_eq!(preview.status, OperationStatus::PreviewReady);
        let expected = if username == "root-sim" {
            "root"
        } else {
            "sudo"
        };
        assert!(preview.permission_summary.to_lowercase().contains(expected));
    }

    let no_privilege = trusted_fixture_as(&services, "no-priv").await;
    assert!(services
        .operation_service()
        .preflight(&no_privilege, request)
        .await
        .is_err());
}

#[tokio::test]
#[ignore = "requires scripts/ssh-fixture.ps1 -Action Start"]
async fn disconnect_during_a_dangerous_change_is_reported_as_uncertain() {
    let data_root = project_root().join(".local/test-data").join(format!(
        "operations-disconnect-live-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&data_root).unwrap();
    let _cleanup = Cleanup(data_root.clone());
    let services = AppServices::open_with_protector(&data_root, Arc::new(XorProtector))
        .await
        .unwrap();
    let server_id = trusted_fixture(&services).await;

    let details = run_hostname(&services, &server_id, "fixture-disconnect").await;
    assert_eq!(
        details.run.status,
        OperationStatus::Uncertain,
        "{details:#?}"
    );
    assert!(details.steps.iter().any(|step| {
        step.phase == OperationPhase::Execute && step.status == OperationStepStatus::Uncertain
    }));
    let restore = services
        .operation_restore_service()
        .list_by_run(details.run.id)
        .await
        .unwrap();
    assert_eq!(restore.len(), 1);
    assert_eq!(
        restore[0].point.status,
        OperationRestorePointStatus::Available
    );
}

#[tokio::test]
#[ignore = "requires scripts/ssh-fixture.ps1 -Action Start"]
async fn ip_change_arms_rollback_before_apply_then_verifies_and_cleans_expired_assets() {
    let data_root = project_root()
        .join(".local/test-data")
        .join(format!("operations-ip-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&data_root).unwrap();
    let _cleanup = Cleanup(data_root.clone());
    let services = AppServices::open_with_protector(&data_root, Arc::new(XorProtector))
        .await
        .unwrap();
    let server_id = trusted_fixture(&services).await;

    let changed = run_dangerous_task(
        &services,
        &server_id,
        "network.ip_change",
        json!({
            "interface":"eth0",
            "cidr":"127.0.0.1/8",
            "gateway":"127.0.0.1",
            "rollbackSeconds":120
        }),
    )
    .await;
    assert_eq!(
        changed.run.status,
        OperationStatus::Succeeded,
        "{changed:#?}"
    );
    assert_eq!(
        fixture_state(&format!("ip-events-{}", changed.run.id)),
        "armed,applied,finalized"
    );

    let restore = services
        .operation_restore_service()
        .list_by_run(changed.run.id)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let expected_remote_asset = format!("qingzhou-recovery/{}", changed.run.id);
    assert_eq!(restore.point.status, OperationRestorePointStatus::Available);
    assert_eq!(
        restore.point.remote_asset_id.as_deref(),
        Some(expected_remote_asset.as_str())
    );
    assert!(data_root
        .join("backups/tasks")
        .join(changed.run.id.to_string())
        .is_dir());

    let options = SqliteConnectOptions::new().filename(data_root.join("app.db"));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::query("UPDATE operation_restore_points SET expires_at=1 WHERE id=?")
        .bind(restore.point.id.to_string())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let cleaned = services
        .operation_restore_service()
        .cleanup_assets(restore.point.id)
        .await
        .unwrap();
    assert_eq!(cleaned.point.status, OperationRestorePointStatus::Expired);
    assert_eq!(
        fixture_state(&format!("ip-events-{}", changed.run.id)),
        "armed,applied,finalized,cleaned"
    );
    assert!(!data_root
        .join("backups/tasks")
        .join(changed.run.id.to_string())
        .exists());
}
