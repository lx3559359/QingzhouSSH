use std::sync::Arc;

use qingzhou_ssh_lib::{
    core::{
        database::Database, secret_protector::SecretProtector, ssh::executor::VecEventSink,
        system_probe::SystemCapabilities, tasks::RiskLevel,
    },
    domain::{
        operation::{OperationStatus, OperationStepStatus},
        server::{CreateServerRequest, CredentialInput, ServerProfile},
    },
    error::AppResult,
    services::{
        app_services::AppServices,
        operation_service::{OperationPreflightRequest, OperationStartRequest},
    },
};
use serde_json::json;

struct XorProtector;

impl SecretProtector for XorProtector {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xA5).collect())
    }

    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xA5).collect())
    }
}

async fn fixture() -> (tempfile::TempDir, AppServices, ServerProfile) {
    let root = tempfile::tempdir().unwrap();
    let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "未信任运维服务器".into(),
            host: "127.0.0.1".into(),
            port: 22,
            username: "tester".into(),
            credential: CredentialInput::Password {
                password: "operation-service-canary".into(),
            },
        })
        .await
        .unwrap();
    (root, services, server)
}

fn capabilities(service_manager: &str, commands: &[&str]) -> SystemCapabilities {
    SystemCapabilities {
        os_id: "openeuler".into(),
        os_family: "openeuler".into(),
        version_id: Some("24.03".into()),
        package_manager: Some("dnf".into()),
        service_manager: service_manager.into(),
        architecture: "x86_64".into(),
        shell: "/bin/sh".into(),
        commands: commands.iter().map(|value| (*value).into()).collect(),
    }
}

#[tokio::test]
async fn preflight_returns_public_plan_without_commands() {
    let (_root, services, server) = fixture().await;
    let preview = services
        .operation_service()
        .preflight_with_capabilities(
            &server.id,
            OperationPreflightRequest {
                task_id: "system.overview".into(),
                task_version: 2,
                parameters: json!({}),
            },
            &capabilities("systemd", &["uptime", "uname", "df", "ps"]),
        )
        .await
        .unwrap();
    let encoded = serde_json::to_string(&preview).unwrap();
    assert_eq!(preview.risk_level, RiskLevel::Safe);
    assert_eq!(preview.status, OperationStatus::PreviewReady);
    for private in ["uname -a", "command", "commandTemplate"] {
        assert!(!encoded.contains(private));
    }
}

#[tokio::test]
async fn unknown_task_is_rejected_before_an_operation_run_is_created() {
    let (root, services, server) = fixture().await;
    let error = services
        .operation_service()
        .preflight(
            &server.id,
            OperationPreflightRequest {
                task_id: "missing.task".into(),
                task_version: 2,
                parameters: json!({}),
            },
        )
        .await;
    assert!(error.is_err());
    let database = Database::open(root.path()).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM operation_runs")
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn safe_run_links_the_existing_execution_even_when_connection_fails() {
    let (_root, services, server) = fixture().await;
    let operations = services.operation_service();
    let request = OperationPreflightRequest {
        task_id: "system.disk_usage".into(),
        task_version: 2,
        parameters: json!({}),
    };
    let capabilities = capabilities("systemd", &["df"]);
    let preview = operations
        .preflight_with_capabilities(&server.id, request.clone(), &capabilities)
        .await
        .unwrap();
    let mut events = VecEventSink::default();
    let details = operations
        .start_with_capabilities(
            &server.id,
            OperationStartRequest {
                task_id: request.task_id,
                task_version: request.task_version,
                parameters: request.parameters,
                confirmed_preview_id: Some(preview.preview_id),
            },
            &capabilities,
            &mut events,
        )
        .await
        .unwrap();
    assert_eq!(details.run.status, OperationStatus::Failed);
    assert_eq!(details.steps[0].status, OperationStepStatus::Failed);
    assert!(details.steps[0].execution_id.is_some());
}

#[tokio::test]
async fn dangerous_run_without_confirmation_creates_no_execution() {
    let (root, services, server) = fixture().await;
    let operations = services.operation_service();
    let capabilities = capabilities("systemd", &["systemctl"]);
    operations
        .preflight_with_capabilities(
            &server.id,
            OperationPreflightRequest {
                task_id: "service.restart".into(),
                task_version: 2,
                parameters: json!({"service":"nginx"}),
            },
            &capabilities,
        )
        .await
        .unwrap();
    let mut events = VecEventSink::default();
    assert!(operations
        .start_with_capabilities(
            &server.id,
            OperationStartRequest {
                task_id: "service.restart".into(),
                task_version: 2,
                parameters: json!({"service":"nginx"}),
                confirmed_preview_id: None,
            },
            &capabilities,
            &mut events,
        )
        .await
        .is_err());

    let database = Database::open(root.path()).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM executions")
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn confirmed_preview_cannot_be_reused_with_different_parameters() {
    let (root, services, server) = fixture().await;
    let operations = services.operation_service();
    let capabilities = capabilities("systemd", &["systemctl"]);
    let preview = operations
        .preflight_with_capabilities(
            &server.id,
            OperationPreflightRequest {
                task_id: "service.restart".into(),
                task_version: 2,
                parameters: json!({"service":"nginx"}),
            },
            &capabilities,
        )
        .await
        .unwrap();
    let mut events = VecEventSink::default();
    assert!(operations
        .start_with_capabilities(
            &server.id,
            OperationStartRequest {
                task_id: "service.restart".into(),
                task_version: 2,
                parameters: json!({"service":"sshd"}),
                confirmed_preview_id: Some(preview.preview_id),
            },
            &capabilities,
            &mut events,
        )
        .await
        .is_err());

    let database = Database::open(root.path()).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM executions")
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn operation_start_contract_rejects_frontend_command_text() {
    let decoded = serde_json::from_value::<OperationStartRequest>(json!({
        "taskId":"system.overview",
        "taskVersion":2,
        "parameters":{},
        "confirmedPreviewId":null,
        "command":"id"
    }));
    assert!(decoded.is_err());
}
