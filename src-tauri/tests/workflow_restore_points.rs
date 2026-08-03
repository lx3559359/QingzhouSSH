use std::{path::Path, sync::Arc};

use qingzhou_ssh_lib::{
    core::{
        database::Database,
        secret_protector::SecretProtector,
        workflows::{resolve_restore_point_path, restore_point_relative_path},
    },
    domain::{
        server::{AuthKind, CreateServerRequest, CredentialInput, ServerProfile},
        workflow::{
            FinishWorkflowRestorePoint, NewWorkflowRestorePoint, NewWorkflowRun, NodePosition,
            WorkflowDraft, WorkflowNode, WorkflowNodeConfig, WorkflowRestorePointStatus,
        },
    },
    error::AppResult,
    repositories::{server_repository::ServerRepository, workflow_repository::WorkflowRepository},
    services::app_services::AppServices,
};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct XorProtector;

struct Cleanup(std::path::PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl SecretProtector for XorProtector {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0x5a).collect())
    }

    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0x5a).collect())
    }
}

fn workflow_draft() -> WorkflowDraft {
    WorkflowDraft {
        id: None,
        name: "restore point test".into(),
        description: String::new(),
        nodes: vec![WorkflowNode {
            id: Uuid::new_v4(),
            name: "start".into(),
            position: NodePosition { x: 0.0, y: 0.0 },
            config: WorkflowNodeConfig::Start {},
        }],
        edges: Vec::new(),
    }
}

async fn repository_harness() -> (
    tempfile::TempDir,
    WorkflowRepository,
    ServerProfile,
    Uuid,
    Uuid,
) {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(root.path()).await.unwrap();
    let server = ServerProfile::new(
        "restore fixture",
        "127.0.0.1",
        22,
        "tester",
        AuthKind::Password,
        "restore-credential",
    );
    ServerRepository::new(database.pool().clone())
        .insert(&server)
        .await
        .unwrap();
    let repository = WorkflowRepository::new(database.pool().clone());
    let workflow = repository.save(workflow_draft()).await.unwrap();
    let run = repository
        .create_run(NewWorkflowRun {
            workflow_id: workflow.id,
            workflow_version: workflow.version,
            server_id: server.id.clone(),
        })
        .await
        .unwrap();
    (root, repository, server, run.id, workflow.nodes[0].id)
}

#[test]
fn restore_paths_are_scoped_and_untrusted_paths_are_rejected() {
    let root = Path::new("D:/Qingzhou/data");
    let run_id = Uuid::new_v4();
    let node_id = Uuid::new_v4();
    let relative = restore_point_relative_path(run_id, node_id, "/etc/my app.conf").unwrap();
    let second = restore_point_relative_path(run_id, node_id, "/etc/my app.conf").unwrap();

    assert!(relative.starts_with(&format!("backups/workflows/{run_id}/{node_id}/")));
    assert_ne!(relative, second);
    assert!(!relative.contains(' '));
    assert_eq!(
        resolve_restore_point_path(root, &relative).unwrap(),
        root.join(relative.replace('/', "\\"))
    );

    for unsafe_path in [
        "../app.db",
        "backups/workflows/run/../../app.db",
        "/backups/workflows/run/node/file",
        "D:/backups/workflows/run/node/file",
        "backups/workflows/run/node/file\0tail",
        "downloads/file",
    ] {
        assert!(resolve_restore_point_path(root, unsafe_path).is_err());
    }
    for unsafe_remote in [
        "etc/app.conf",
        "/etc/../app.conf",
        "/etc/app\0.conf",
        "/etc/",
    ] {
        assert!(restore_point_relative_path(run_id, node_id, unsafe_remote).is_err());
    }
}

#[tokio::test]
async fn restore_point_lifecycle_is_persisted_with_run_details() {
    let (_root, repository, _server, run_id, node_id) = repository_harness().await;
    let relative = restore_point_relative_path(run_id, node_id, "/etc/app.conf").unwrap();
    let creating = repository
        .create_restore_point(NewWorkflowRestorePoint {
            run_id,
            node_id,
            remote_path: "/etc/app.conf".into(),
            relative_path: Some(relative.clone()),
            applicability: json!({"serverId": "fixture", "operation": "restore"}),
        })
        .await
        .unwrap();
    assert_eq!(creating.status, WorkflowRestorePointStatus::Creating);

    repository
        .finish_restore_point(FinishWorkflowRestorePoint {
            id: creating.id,
            status: WorkflowRestorePointStatus::Available,
            original_existed: true,
            relative_path: Some(relative),
            size_bytes: Some(128),
            sha256: Some("a".repeat(64)),
            error_message: None,
        })
        .await
        .unwrap();

    let details = repository.get_run(run_id).await.unwrap().unwrap();
    assert_eq!(details.restore_points.len(), 1);
    let saved = &details.restore_points[0];
    assert_eq!(saved.status, WorkflowRestorePointStatus::Available);
    assert!(saved.original_existed);
    assert_eq!(saved.size_bytes, Some(128));
    assert_eq!(
        saved.sha256.as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
}

#[tokio::test]
async fn failed_capture_is_recorded_without_leaving_a_partial_file() {
    let root = tempfile::tempdir().unwrap();
    let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "untrusted".into(),
            host: "127.0.0.1".into(),
            port: 2222,
            username: "testuser".into(),
            credential: CredentialInput::Password {
                password: "testpass".into(),
            },
        })
        .await
        .unwrap();
    let workflow = services
        .workflow_repository()
        .save(workflow_draft())
        .await
        .unwrap();
    let run = services
        .workflow_repository()
        .create_run(NewWorkflowRun {
            workflow_id: workflow.id,
            workflow_version: workflow.version,
            server_id: server.id.clone(),
        })
        .await
        .unwrap();
    let node_id = workflow.nodes[0].id;

    let result = services
        .restore_point_service()
        .capture(
            run.id,
            node_id,
            &server.id,
            "/tmp/app.conf",
            CancellationToken::new(),
        )
        .await;
    assert!(result.is_err());

    let details = services
        .workflow_repository()
        .get_run(run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(details.restore_points.len(), 1);
    assert_eq!(
        details.restore_points[0].status,
        WorkflowRestorePointStatus::Failed
    );
    assert!(details.restore_points[0].error_message.is_some());
    let backup_root = root
        .path()
        .join("backups")
        .join("workflows")
        .join(run.id.to_string())
        .join(node_id.to_string());
    assert!(!backup_root.exists() || std::fs::read_dir(backup_root).unwrap().next().is_none());
}

#[tokio::test]
#[ignore = "requires scripts/ssh-fixture.ps1 -Action Start"]
async fn live_capture_handles_existing_missing_and_cancelled_remote_files() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap();
    let data_root = project_root
        .join(".local/test-data")
        .join(format!("workflow-restore-live-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&data_root).unwrap();
    let _cleanup = Cleanup(data_root.clone());
    let services = AppServices::open_with_protector(&data_root, Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "restore live".into(),
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
    let workflow = services
        .workflow_repository()
        .save(workflow_draft())
        .await
        .unwrap();
    let run = services
        .workflow_repository()
        .create_run(NewWorkflowRun {
            workflow_id: workflow.id,
            workflow_version: workflow.version,
            server_id: server.id.clone(),
        })
        .await
        .unwrap();
    let node_id = workflow.nodes[0].id;
    let suffix = Uuid::new_v4();
    let existing = format!("/tmp/qingzhou-restore-existing-{suffix}.bin");
    let missing = format!("/tmp/qingzhou-restore-missing-{suffix}.bin");
    let cancelled = format!("/tmp/qingzhou-restore-cancelled-{suffix}.bin");
    let fixture_temp = project_root.join(".local/ssh-fixture/remote-root/tmp");
    let existing_fixture = fixture_temp.join(existing.rsplit('/').next().unwrap());
    let cancelled_fixture = fixture_temp.join(cancelled.rsplit('/').next().unwrap());
    tokio::fs::write(&existing_fixture, b"restore-payload")
        .await
        .unwrap();
    let large_file = std::fs::File::create(&cancelled_fixture).unwrap();
    large_file.set_len(8 * 1024 * 1024).unwrap();
    drop(large_file);

    let saved = services
        .restore_point_service()
        .capture(
            run.id,
            node_id,
            &server.id,
            &existing,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(saved.original_existed);
    assert_eq!(saved.size_bytes, Some(15));
    assert_eq!(
        tokio::fs::read(
            resolve_restore_point_path(&data_root, saved.relative_path.as_deref().unwrap())
                .unwrap()
        )
        .await
        .unwrap(),
        b"restore-payload"
    );

    let absent = services
        .restore_point_service()
        .capture(
            run.id,
            node_id,
            &server.id,
            &missing,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!absent.original_existed);
    assert!(absent.relative_path.is_none());

    let cancel = CancellationToken::new();
    cancel.cancel();
    let cancelled_result = services
        .restore_point_service()
        .capture(run.id, node_id, &server.id, &cancelled, cancel)
        .await;
    assert!(matches!(
        cancelled_result,
        Err(qingzhou_ssh_lib::error::AppError::Cancelled)
    ));
    let node_backup = data_root
        .join("backups/workflows")
        .join(run.id.to_string())
        .join(node_id.to_string());
    if node_backup.exists() {
        assert!(std::fs::read_dir(node_backup).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".partial")));
    }

    tokio::fs::remove_file(existing_fixture).await.unwrap();
    tokio::fs::remove_file(cancelled_fixture).await.unwrap();
}
