use std::{collections::BTreeMap, sync::Arc};

use qingzhou_ssh_lib::{
    core::{secret_protector::SecretProtector, ssh::executor::VecEventSink},
    domain::{
        server::{CreateServerRequest, CredentialInput},
        workflow::{WorkflowCustomMode, WorkflowNodeConfig, WorkflowNodeStatus},
    },
    error::AppResult,
    services::{app_services::AppServices, workflow_nodes::execution::ExecutionNodeAdapter},
};

struct XorProtector;

impl SecretProtector for XorProtector {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xA5).collect())
    }

    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xA5).collect())
    }
}

#[tokio::test]
async fn task_and_custom_nodes_reuse_m2_executions_and_return_linkable_outcomes() {
    let root = tempfile::tempdir().unwrap();
    let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "未信任工作流服务器".into(),
            host: "127.0.0.1".into(),
            port: 22,
            username: "tester".into(),
            credential: CredentialInput::Password {
                password: "workflow-node-password-canary".into(),
            },
        })
        .await
        .unwrap();
    let adapter = ExecutionNodeAdapter::new(services.execution_service());

    let mut task_events = VecEventSink::default();
    let task = adapter
        .execute(
            &server.id,
            &WorkflowNodeConfig::Task {
                task_id: "system.disk_usage".into(),
                task_version: 1,
                parameters: BTreeMap::new(),
            },
            true,
            &mut task_events,
        )
        .await
        .unwrap();
    assert_eq!(task.status, WorkflowNodeStatus::Failed);
    assert_eq!(task.task_id, "system.disk_usage");
    assert!(task.execution_id != uuid::Uuid::nil());
    assert_eq!(task.error_category.as_deref(), Some("security"));

    let script_canary = "workflow-script-sensitive-canary";
    let mut custom_events = VecEventSink::default();
    let custom = adapter
        .execute(
            &server.id,
            &WorkflowNodeConfig::Custom {
                mode: WorkflowCustomMode::Script,
                content: format!("printf '%s\\n' '{script_canary}'"),
                timeout_seconds: 30,
            },
            true,
            &mut custom_events,
        )
        .await
        .unwrap();
    assert_eq!(custom.status, WorkflowNodeStatus::Failed);
    assert_eq!(custom.task_id, "advanced.script");
    assert!(custom.result.is_none());

    let database = std::fs::read(root.path().join("app.db")).unwrap();
    let encoded = String::from_utf8_lossy(&database);
    assert!(!encoded.contains("workflow-node-password-canary"));
    assert!(!encoded.contains(script_canary));
    assert!(!serde_json::to_string(&custom_events.events)
        .unwrap()
        .contains(script_canary));
}

#[tokio::test]
async fn adapter_rejects_non_execution_nodes_and_missing_confirmation() {
    let root = tempfile::tempdir().unwrap();
    let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let adapter = ExecutionNodeAdapter::new(services.execution_service());
    let mut events = VecEventSink::default();
    assert!(adapter
        .execute(
            "server",
            &WorkflowNodeConfig::Stop {
                message: "stop".into(),
            },
            true,
            &mut events,
        )
        .await
        .is_err());
    assert!(adapter
        .execute(
            "server",
            &WorkflowNodeConfig::Custom {
                mode: WorkflowCustomMode::Command,
                content: "uptime".into(),
                timeout_seconds: 30,
            },
            false,
            &mut events,
        )
        .await
        .is_err());
}
