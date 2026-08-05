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

async fn trusted_fixture(services: &AppServices) -> String {
    let server = services
        .create_server(CreateServerRequest {
            name: "危险任务恢复夹具".into(),
            host: "127.0.0.1".into(),
            port: 2222,
            username: "testuser".into(),
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
