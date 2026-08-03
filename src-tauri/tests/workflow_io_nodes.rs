use std::sync::Arc;

use qingzhou_ssh_lib::{
    core::{secret_protector::SecretProtector, ssh::executor::VecEventSink},
    domain::{
        server::{CreateServerRequest, CredentialInput},
        workflow::{WorkflowNodeConfig, WorkflowNodeStatus},
    },
    error::AppResult,
    services::{app_services::AppServices, workflow_nodes::io::IoNodeAdapter},
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
async fn log_upload_and_download_nodes_reuse_m2_services_and_execution_history() {
    let root = tempfile::tempdir().unwrap();
    let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "IO 节点服务器".into(),
            host: "127.0.0.1".into(),
            port: 22,
            username: "tester".into(),
            credential: CredentialInput::Password {
                password: "workflow-io-password-canary".into(),
            },
        })
        .await
        .unwrap();
    let adapter = IoNodeAdapter::new(services.log_service(), services.transfer_service());

    let mut events = VecEventSink::default();
    let log = adapter
        .execute(
            &server.id,
            &WorkflowNodeConfig::LogSearch {
                path: "/var/log/qingzhou.log".into(),
                keyword: "error".into(),
                case_sensitive: false,
                context_lines: 2,
                limit: 100,
                start_time: None,
                end_time: None,
            },
            &mut events,
        )
        .await
        .unwrap();
    assert_eq!(log.task_id, "logs.search");
    assert_eq!(log.status, WorkflowNodeStatus::Failed);
    assert!(log.execution_id != uuid::Uuid::nil());

    let local_path = root.path().join("release.zip");
    std::fs::write(&local_path, b"release").unwrap();
    let upload = adapter
        .execute(
            &server.id,
            &WorkflowNodeConfig::Upload {
                local_path: local_path.to_string_lossy().into_owned(),
                remote_path: "/tmp/release.zip".into(),
                overwrite: false,
                create_restore_point: false,
            },
            &mut events,
        )
        .await
        .unwrap();
    assert_eq!(upload.task_id, "transfer.upload");
    assert_eq!(upload.status, WorkflowNodeStatus::Failed);

    let download = adapter
        .execute(
            &server.id,
            &WorkflowNodeConfig::Download {
                remote_path: "/tmp/result.zip".into(),
                suggested_name: "result.zip".into(),
                overwrite: false,
            },
            &mut events,
        )
        .await
        .unwrap();
    assert_eq!(download.task_id, "transfer.download");
    assert_eq!(download.status, WorkflowNodeStatus::Failed);

    let database =
        String::from_utf8_lossy(&std::fs::read(root.path().join("app.db")).unwrap()).into_owned();
    assert!(!database.contains("workflow-io-password-canary"));
    assert_eq!(
        services
            .list_executions(Default::default())
            .await
            .unwrap()
            .len(),
        3
    );
}

#[tokio::test]
async fn io_adapter_rejects_non_io_nodes_before_creating_history() {
    let root = tempfile::tempdir().unwrap();
    let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let adapter = IoNodeAdapter::new(services.log_service(), services.transfer_service());
    let mut events = VecEventSink::default();
    assert!(adapter
        .execute(
            "server",
            &WorkflowNodeConfig::Stop {
                message: "stop".into(),
            },
            &mut events,
        )
        .await
        .is_err());
    assert!(services
        .list_executions(Default::default())
        .await
        .unwrap()
        .is_empty());
}
