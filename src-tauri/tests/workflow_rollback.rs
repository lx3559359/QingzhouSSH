use std::{path::Path, sync::Arc, time::Duration};

use qingzhou_ssh_lib::{
    core::{
        database::Database, secret_protector::SecretProtector, sftp::sha256_local_file,
        workflows::restore_point_relative_path,
    },
    domain::{
        execution::now_millis,
        server::{CreateServerRequest, CredentialInput, StoredHostKey},
        workflow::{
            FinishWorkflowNode, FinishWorkflowRestorePoint, FinishWorkflowRun,
            NewWorkflowRestorePoint, NewWorkflowRun, NodePosition, WorkflowCustomMode,
            WorkflowDraft, WorkflowEdge, WorkflowEdgeBranch, WorkflowNode, WorkflowNodeConfig,
            WorkflowNodeStatus, WorkflowRestorePointStatus, WorkflowRunStatus,
        },
    },
    error::AppResult,
    repositories::server_repository::ServerRepository,
    services::app_services::AppServices,
};
use serde_json::json;
use uuid::Uuid;

struct XorProtector;

impl SecretProtector for XorProtector {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xa5).collect())
    }

    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xa5).collect())
    }
}

async fn services() -> (tempfile::TempDir, AppServices, String) {
    let root = tempfile::tempdir().unwrap();
    let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "rollback server".into(),
            host: "127.0.0.1".into(),
            port: 9,
            username: "tester".into(),
            credential: CredentialInput::Password {
                password: "rollback-password-canary".into(),
            },
        })
        .await
        .unwrap();
    let database = Database::open(root.path()).await.unwrap();
    ServerRepository::new(database.pool().clone())
        .upsert_host_key(&StoredHostKey {
            server_id: server.id.clone(),
            algorithm: "ssh-ed25519".into(),
            fingerprint_sha256: "SHA256:fixture".into(),
            raw_key_base64: "fixture".into(),
        })
        .await
        .unwrap();
    (root, services, server.id)
}

fn diagnostic_workflow(script_canary: &str) -> WorkflowDraft {
    let start = Uuid::new_v4();
    let custom = Uuid::new_v4();
    let stop = Uuid::new_v4();
    WorkflowDraft {
        id: None,
        name: "diagnostic workflow".into(),
        description: "safe metadata".into(),
        nodes: vec![
            WorkflowNode {
                id: start,
                name: "start".into(),
                position: NodePosition { x: 0.0, y: 0.0 },
                config: WorkflowNodeConfig::Start {},
            },
            WorkflowNode {
                id: custom,
                name: "custom".into(),
                position: NodePosition { x: 100.0, y: 0.0 },
                config: WorkflowNodeConfig::Custom {
                    mode: WorkflowCustomMode::Script,
                    content: format!("printf '{script_canary}'"),
                    timeout_seconds: 30,
                },
            },
            WorkflowNode {
                id: stop,
                name: "stop".into(),
                position: NodePosition { x: 200.0, y: 0.0 },
                config: WorkflowNodeConfig::Stop {
                    message: "done".into(),
                },
            },
        ],
        edges: vec![
            WorkflowEdge {
                from: start,
                to: custom,
                branch: WorkflowEdgeBranch::Success,
            },
            WorkflowEdge {
                from: custom,
                to: stop,
                branch: WorkflowEdgeBranch::Success,
            },
        ],
    }
}

async fn paused_run(services: &AppServices, server_id: &str) -> (Uuid, Uuid) {
    let repository = services.workflow_repository();
    let definition = repository
        .save(diagnostic_workflow("diagnostic-script-canary"))
        .await
        .unwrap();
    let failed_node = definition.nodes[1].id;
    let run = repository
        .create_run(NewWorkflowRun {
            workflow_id: definition.id,
            workflow_version: definition.version,
            server_id: server_id.into(),
        })
        .await
        .unwrap();
    repository
        .mark_run_running(run.id, failed_node, now_millis())
        .await
        .unwrap();
    let attempt = repository
        .start_node_attempt(run.id, failed_node, now_millis())
        .await
        .unwrap();
    repository
        .finish_node(FinishWorkflowNode {
            run_id: run.id,
            node_id: failed_node,
            attempt: attempt.attempt,
            status: WorkflowNodeStatus::Failed,
            finished_at: now_millis(),
            exit_code: None,
            result: None,
            output_summary: None,
            error_message: Some("password=diagnostic-error-canary".into()),
            retryable: false,
        })
        .await
        .unwrap();
    repository
        .finish_run(FinishWorkflowRun {
            run_id: run.id,
            status: WorkflowRunStatus::Paused,
            finished_at: now_millis(),
            error_category: Some("injected".into()),
            error_message: Some("password=diagnostic-error-canary".into()),
            retryable: false,
        })
        .await
        .unwrap();
    (run.id, failed_node)
}

async fn available_restore_point(
    services: &AppServices,
    run_id: Uuid,
    node_id: Uuid,
    remote_path: &str,
    payload: &[u8],
) -> (Uuid, String) {
    let relative = restore_point_relative_path(run_id, node_id, remote_path).unwrap();
    let absolute = services.data_root().join(&relative);
    std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
    std::fs::write(&absolute, payload).unwrap();
    let sha256 = sha256_local_file(&absolute).await.unwrap();
    let repository = services.workflow_repository();
    let point = repository
        .create_restore_point(NewWorkflowRestorePoint {
            run_id,
            node_id,
            remote_path: remote_path.into(),
            relative_path: Some(relative.clone()),
            applicability: json!({
                "serverId": repository.get_run(run_id).await.unwrap().unwrap().run.server_id,
                "remotePath": remote_path,
                "strategy": "restoreExistingOrDeleteCreated"
            }),
        })
        .await
        .unwrap();
    repository
        .finish_restore_point(FinishWorkflowRestorePoint {
            id: point.id,
            status: WorkflowRestorePointStatus::Available,
            original_existed: true,
            relative_path: Some(relative.clone()),
            size_bytes: Some(payload.len() as u64),
            sha256: Some(sha256),
            error_message: None,
        })
        .await
        .unwrap();
    (point.id, relative)
}

#[tokio::test]
async fn rollback_requires_dangerous_confirmation_before_any_mutation() {
    let (_root, services, server_id) = services().await;
    let (run_id, node_id) = paused_run(&services, &server_id).await;
    available_restore_point(
        &services,
        run_id,
        node_id,
        "/etc/rollback.conf",
        b"original",
    )
    .await;

    assert!(services
        .restore_point_service()
        .rollback_run(run_id, false)
        .await
        .is_err());
    let details = services
        .workflow_repository()
        .get_run(run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(details.run.status, WorkflowRunStatus::Paused);
    assert_eq!(
        details.restore_points[0].status,
        WorkflowRestorePointStatus::Available
    );
}

#[tokio::test]
async fn cleanup_deletes_only_registered_files_and_is_idempotent() {
    let (_root, services, server_id) = services().await;
    let (run_id, node_id) = paused_run(&services, &server_id).await;
    let (_point_id, relative) = available_restore_point(
        &services,
        run_id,
        node_id,
        "/etc/cleanup.conf",
        b"registered",
    )
    .await;
    let registered = services.data_root().join(relative);
    let sentinel = registered.parent().unwrap().join("unregistered.keep");
    std::fs::write(&sentinel, b"keep").unwrap();

    assert_eq!(
        services
            .restore_point_service()
            .cleanup_run(run_id)
            .await
            .unwrap(),
        1
    );
    assert!(!registered.exists());
    assert!(sentinel.exists());
    assert_eq!(
        services
            .restore_point_service()
            .cleanup_run(run_id)
            .await
            .unwrap(),
        0
    );
    let details = services
        .workflow_repository()
        .get_run(run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        details.restore_points[0].status,
        WorkflowRestorePointStatus::Expired
    );

    let repository = services.workflow_repository();
    let definition = repository
        .save(diagnostic_workflow("running"))
        .await
        .unwrap();
    let running = repository
        .create_run(NewWorkflowRun {
            workflow_id: definition.id,
            workflow_version: definition.version,
            server_id,
        })
        .await
        .unwrap();
    repository
        .mark_run_running(running.id, definition.nodes[0].id, now_millis())
        .await
        .unwrap();
    assert!(services
        .restore_point_service()
        .cleanup_run(running.id)
        .await
        .is_err());
}

#[tokio::test]
async fn diagnostics_are_project_local_complete_and_redacted() {
    let (_root, services, server_id) = services().await;
    let (run_id, node_id) = paused_run(&services, &server_id).await;
    available_restore_point(
        &services,
        run_id,
        node_id,
        "/etc/diagnostic.conf",
        b"diagnostic-backup",
    )
    .await;

    let file = services
        .workflow_diagnostics_service()
        .export(run_id)
        .await
        .unwrap();
    assert!(file.relative_path.starts_with("downloads/"));
    let absolute = services.data_root().join(&file.relative_path);
    assert_eq!(file.sha256, sha256_local_file(&absolute).await.unwrap());
    assert_eq!(file.size_bytes, std::fs::metadata(&absolute).unwrap().len());
    let encoded = std::fs::read_to_string(absolute).unwrap();
    assert!(encoded.contains("timeline"));
    assert!(encoded.contains("restorePoints"));
    assert!(encoded.contains("checksumSha256"));
    assert!(!encoded.contains("diagnostic-script-canary"));
    assert!(!encoded.contains("diagnostic-error-canary"));
    assert!(!encoded.contains("rollback-password-canary"));
    assert!(encoded.contains("[REDACTED]"));
}

#[tokio::test]
#[ignore = "requires scripts/ssh-fixture.ps1 -Action Start"]
async fn live_rollback_is_reverse_ordered_deletes_created_files_and_reports_partial_failure() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap();
    let data_root = project_root
        .join(".local/test-data")
        .join(format!("workflow-rollback-live-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&data_root).unwrap();
    let services = AppServices::open_with_protector(&data_root, Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "rollback live".into(),
            host: "127.0.0.1".into(),
            port: 2222,
            username: "testuser".into(),
            credential: CredentialInput::Password {
                password: "testpass".into(),
            },
        })
        .await
        .unwrap();
    let observation = services
        .inspect_host_key(&server.id)
        .await
        .unwrap()
        .observed;
    services
        .trust_host_key(&server.id, observation)
        .await
        .unwrap();
    let fixture_temp = project_root.join(".local/ssh-fixture/remote-root/tmp");
    let suffix = Uuid::new_v4();
    let remote = format!("/tmp/qingzhou-rollback-order-{suffix}.conf");
    let created_remote = format!("/tmp/qingzhou-rollback-created-{suffix}.conf");
    let remote_file = fixture_temp.join(remote.rsplit('/').next().unwrap());
    let created_file = fixture_temp.join(created_remote.rsplit('/').next().unwrap());
    let (run_id, node_id) = paused_run(&services, &server.id).await;
    available_restore_point(&services, run_id, node_id, &remote, b"base").await;
    tokio::time::sleep(Duration::from_millis(2)).await;
    available_restore_point(&services, run_id, node_id, &remote, b"middle").await;
    let repository = services.workflow_repository();
    let create_point = repository
        .create_restore_point(NewWorkflowRestorePoint {
            run_id,
            node_id,
            remote_path: created_remote.clone(),
            relative_path: None,
            applicability: json!({
                "serverId": server.id,
                "remotePath": created_remote,
                "strategy": "restoreExistingOrDeleteCreated"
            }),
        })
        .await
        .unwrap();
    repository
        .finish_restore_point(FinishWorkflowRestorePoint {
            id: create_point.id,
            status: WorkflowRestorePointStatus::Available,
            original_existed: false,
            relative_path: None,
            size_bytes: None,
            sha256: None,
            error_message: None,
        })
        .await
        .unwrap();
    std::fs::write(&remote_file, b"final").unwrap();
    std::fs::write(&created_file, b"created-by-run").unwrap();

    let rolled_back = services
        .restore_point_service()
        .rollback_run(run_id, true)
        .await
        .unwrap();
    assert_eq!(rolled_back.run.status, WorkflowRunStatus::RolledBack);
    assert_eq!(std::fs::read(&remote_file).unwrap(), b"base");
    assert!(!created_file.exists());
    assert!(rolled_back
        .restore_points
        .iter()
        .all(|point| point.status == WorkflowRestorePointStatus::RolledBack));

    let bad_remote = format!("/tmp/qingzhou-rollback-bad-{suffix}.conf");
    let good_remote = format!("/tmp/qingzhou-rollback-good-{suffix}.conf");
    let bad_file = fixture_temp.join(bad_remote.rsplit('/').next().unwrap());
    let good_file = fixture_temp.join(good_remote.rsplit('/').next().unwrap());
    let (failed_run, failed_node) = paused_run(&services, &server.id).await;
    let (_bad_id, bad_relative) = available_restore_point(
        &services,
        failed_run,
        failed_node,
        &bad_remote,
        b"bad-original",
    )
    .await;
    available_restore_point(
        &services,
        failed_run,
        failed_node,
        &good_remote,
        b"good-original",
    )
    .await;
    std::fs::write(services.data_root().join(bad_relative), b"tampered").unwrap();
    std::fs::write(&bad_file, b"bad-mutated").unwrap();
    std::fs::write(&good_file, b"good-mutated").unwrap();

    let partial = services
        .restore_point_service()
        .rollback_run(failed_run, true)
        .await
        .unwrap();
    assert_eq!(partial.run.status, WorkflowRunStatus::RollbackFailed);
    assert_eq!(std::fs::read(&bad_file).unwrap(), b"bad-mutated");
    assert_eq!(std::fs::read(&good_file).unwrap(), b"good-original");
    assert!(partial
        .restore_points
        .iter()
        .any(|point| point.status == WorkflowRestorePointStatus::Failed));
    assert!(partial
        .restore_points
        .iter()
        .any(|point| point.status == WorkflowRestorePointStatus::RolledBack));

    for path in [remote_file, created_file, bad_file, good_file] {
        if path.exists() {
            std::fs::remove_file(path).unwrap();
        }
    }
    drop(repository);
    drop(services);
    let mut removed = false;
    for _ in 0..20 {
        match std::fs::remove_dir_all(&data_root) {
            Ok(()) => {
                removed = true;
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                removed = true;
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    assert!(removed, "live rollback data root remained locked");
}
