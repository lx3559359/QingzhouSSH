use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use qingzhou_ssh_lib::{
    core::tasks::{
        resolve_task_restore_path, task_restore_dir, task_restore_item_relative_path,
        validate_restore_relative_path, write_restore_asset_atomic, BackupItemDefinition,
        BackupItemKind, BackupPlan, ValidatedParameters,
    },
    core::{
        database::Database, secret_protector::SecretProtector, system_probe::SystemCapabilities,
    },
    domain::{
        operation::{NewOperationRun, OperationStatus},
        operation_restore::{
            NewOperationRestoreItem, NewOperationRestorePoint, OperationRestoreItemStatus,
            OperationRestorePointStatus,
        },
        server::{AuthKind, CreateServerRequest, CredentialInput, ServerProfile},
    },
    error::AppResult,
    repositories::{
        operation_repository::OperationRepository,
        operation_restore_repository::OperationRestoreRepository,
        server_repository::ServerRepository,
    },
    services::{
        app_services::AppServices, operation_restore_service::build_snapshot_rollback_command,
        operation_service::OperationPreflightRequest,
    },
};
use serde_json::json;
use uuid::Uuid;

struct XorProtector;

impl SecretProtector for XorProtector {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0x3c).collect())
    }

    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0x3c).collect())
    }
}

#[test]
fn task_backup_paths_are_confined_to_the_project_data_root() {
    let run_id = Uuid::nil();
    assert_eq!(
        task_restore_dir(run_id),
        PathBuf::from("backups/tasks/00000000-0000-0000-0000-000000000000")
    );
    let relative = task_restore_item_relative_path(run_id, 0, "/etc/hosts").unwrap();
    assert!(relative.starts_with(task_restore_dir(run_id)));
    assert_eq!(
        resolve_task_restore_path(Path::new("D:/Qingzhou/data"), &relative).unwrap(),
        Path::new("D:/Qingzhou/data").join(relative)
    );

    for unsafe_path in [
        Path::new("../../escape"),
        Path::new("C:/escape"),
        Path::new("backups/tasks/not-a-uuid/file"),
        Path::new("backups/tasks/00000000-0000-0000-0000-000000000000/file.partial"),
        Path::new("backups/workflows/00000000-0000-0000-0000-000000000000/file"),
    ] {
        assert!(validate_restore_relative_path(unsafe_path).is_err());
    }
}

#[test]
fn task5_snapshots_build_fixed_rollback_and_verification_commands() {
    let hostname = build_snapshot_rollback_command(
        "system.hostname_change",
        "stdout:\nhostname=node-1\nstderr:\n",
    )
    .unwrap();
    assert!(hostname.command.contains("set-hostname"));
    assert!(hostname.verify.contains("node-1"));

    let timezone = build_snapshot_rollback_command(
        "system.timezone_change",
        "stdout:\ntimezone=Asia/Shanghai\nstderr:\n",
    )
    .unwrap();
    assert!(timezone.command.contains("set-timezone"));

    let permissions = build_snapshot_rollback_command(
        "security.file_permissions",
        "stdout:\npath=/etc/nginx/nginx.conf\nuid=100\ngid=101\nmode=640\nstderr:\n",
    )
    .unwrap();
    assert!(permissions.command.contains("chown -- 100:101"));
    assert!(permissions.command.contains("chmod -- 0640"));
    assert!(!permissions.command.contains("-R"));

    let swap = build_snapshot_rollback_command(
        "storage.swap_manage",
        "stdout:\npath=/swapfile\nexists=true\nactive=false\nsize=1073741824\nstderr:\n",
    )
    .unwrap();
    assert!(swap.command.contains("1073741824"));
    assert!(swap.verify.contains("/swapfile"));
}

#[test]
fn rollback_snapshot_data_cannot_escape_task5_safety_boundaries() {
    for snapshot in [
        "stdout:\npath=/\nuid=0\ngid=0\nmode=755\nstderr:\n",
        "stdout:\npath=/tmp/swapfile\nexists=false\nactive=false\nsize=0\nstderr:\n",
        "stdout:\ntimezone=Asia/Shanghai;id\nstderr:\n",
        "stdout:\nhostname=node;id\nstderr:\n",
    ] {
        let task_id = if snapshot.contains("uid=") {
            "security.file_permissions"
        } else if snapshot.contains("exists=") {
            "storage.swap_manage"
        } else if snapshot.contains("timezone=") {
            "system.timezone_change"
        } else {
            "system.hostname_change"
        };
        assert!(build_snapshot_rollback_command(task_id, snapshot).is_err());
    }
}

#[test]
fn task6_snapshots_restore_exact_service_and_container_states() {
    let running_service = build_snapshot_rollback_command(
        "service.stop",
        "stdout:\nmanager=systemd\nservice=nginx.service\nactive=active\nenabled=enabled\nstderr:\n",
    )
    .unwrap();
    assert!(running_service.command.contains("systemctl start"));
    assert!(running_service.verify.contains("is-active"));

    let stopped_service = build_snapshot_rollback_command(
        "service.start",
        "stdout:\nmanager=service\nservice=nginx\nactive=inactive\nenabled=unsupported\nstderr:\n",
    )
    .unwrap();
    assert!(stopped_service.command.contains("service 'nginx' stop"));

    let masked_policy = build_snapshot_rollback_command(
        "service.boot_policy",
        "stdout:\nmanager=systemd\nservice=nginx.service\nactive=inactive\nenabled=masked-runtime\nstderr:\n",
    )
    .unwrap();
    assert!(masked_policy.command.contains("mask --runtime"));
    assert!(masked_policy.verify.contains("masked-runtime"));

    let paused_container = build_snapshot_rollback_command(
        "container.action",
        "stdout:\nruntime=podman\ncontainer=web\nstate=paused\nstderr:\n",
    )
    .unwrap();
    assert!(paused_container.command.contains("podman pause"));
    assert!(paused_container.verify.contains("paused"));

    let exited_container = build_snapshot_rollback_command(
        "container.action",
        "stdout:\nruntime=docker\ncontainer=web\nstate=exited\nstderr:\n",
    )
    .unwrap();
    assert!(exited_container.command.contains("docker stop"));
}

#[test]
fn task6_snapshots_reject_unrestorable_or_untrusted_states() {
    for (task_id, snapshot) in [
        (
            "service.restart",
            "stdout:\nmanager=systemd\nservice=nginx;id\nactive=active\nenabled=enabled\nstderr:\n",
        ),
        (
            "service.restart",
            "stdout:\nmanager=systemd\nservice=nginx\nactive=failed\nenabled=enabled\nstderr:\n",
        ),
        (
            "service.boot_policy",
            "stdout:\nmanager=systemd\nservice=nginx\nactive=active\nenabled=evil;id\nstderr:\n",
        ),
        (
            "service.boot_policy",
            "stdout:\nmanager=systemd\nservice=nginx\nactive=unknown\nenabled=enabled\nstderr:\n",
        ),
        (
            "container.action",
            "stdout:\nruntime=docker;id\ncontainer=web\nstate=running\nstderr:\n",
        ),
        (
            "container.action",
            "stdout:\nruntime=docker\ncontainer=web$(id)\nstate=running\nstderr:\n",
        ),
        (
            "container.action",
            "stdout:\nruntime=docker\ncontainer=web\nstate=created\nstderr:\n",
        ),
    ] {
        assert!(build_snapshot_rollback_command(task_id, snapshot).is_err());
    }
}

#[tokio::test]
async fn atomic_restore_asset_is_hashed_and_never_exposes_a_partial_as_available() {
    let root = tempfile::tempdir().unwrap();
    let run_id = Uuid::new_v4();
    let relative = task_restore_item_relative_path(run_id, 0, "/etc/hosts").unwrap();
    let asset = write_restore_asset_atomic(root.path(), &relative, b"127.0.0.1 localhost\n")
        .await
        .unwrap();
    assert_eq!(
        asset.relative_path,
        relative.to_string_lossy().replace('\\', "/")
    );
    assert_eq!(asset.bytes, 20);
    assert_eq!(asset.sha256.len(), 64);
    assert!(root.path().join(relative).is_file());
    assert!(
        std::fs::read_dir(root.path().join(task_restore_dir(run_id)))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".partial"))
    );
}

#[tokio::test]
async fn failed_operation_capture_is_persisted_without_creating_local_assets() {
    let root = tempfile::tempdir().unwrap();
    let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "untrusted operation restore".into(),
            host: "127.0.0.1".into(),
            port: 2222,
            username: "tester".into(),
            credential: CredentialInput::Password {
                password: "restore-canary".into(),
            },
        })
        .await
        .unwrap();
    let preview = services
        .operation_service()
        .preflight_with_capabilities(
            &server.id,
            OperationPreflightRequest {
                task_id: "service.restart".into(),
                task_version: 2,
                parameters: json!({"service": "nginx"}),
            },
            &SystemCapabilities {
                os_id: "openeuler".into(),
                os_family: "openeuler".into(),
                version_id: Some("24.03".into()),
                package_manager: Some("dnf".into()),
                service_manager: "systemd".into(),
                architecture: "x86_64".into(),
                shell: "/bin/sh".into(),
                commands: vec!["systemctl".into()],
                services: vec!["nginx".into()],
                containers: Vec::new(),
            },
        )
        .await
        .unwrap();
    let result = services
        .operation_restore_service()
        .capture(
            preview.preview_id,
            &server.id,
            "service.restart",
            "systemd-restart",
            &BackupPlan {
                items: vec![BackupItemDefinition {
                    id: "service-state".into(),
                    kind: BackupItemKind::RuntimeState,
                    target_template: "systemctl is-active -- nginx".into(),
                }],
            },
            &ValidatedParameters::default(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await;
    assert!(result.is_err());

    let points = services
        .operation_restore_service()
        .list_by_run(preview.preview_id)
        .await
        .unwrap();
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].point.status, OperationRestorePointStatus::Failed);
    assert!(points[0].items.is_empty());
    assert!(!root
        .path()
        .join(task_restore_dir(preview.preview_id))
        .exists());
}

struct Fixture {
    _root: tempfile::TempDir,
    database: Database,
    repository: OperationRestoreRepository,
    run_id: Uuid,
    server_id: String,
}

async fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(root.path()).await.unwrap();
    let servers = ServerRepository::new(database.pool().clone());
    let server = ServerProfile::new(
        "restore fixture",
        "127.0.0.1",
        22,
        "tester",
        AuthKind::Password,
        "restore-credential",
    );
    servers.insert(&server).await.unwrap();
    let operations = OperationRepository::new(database.pool().clone());
    let run = operations
        .create(NewOperationRun {
            server_id: server.id.clone(),
            task_id: "system.hostname.set".into(),
            task_version: 2,
            risk_level: qingzhou_ssh_lib::core::tasks::RiskLevel::Dangerous,
            parameters_summary: Some("hostname=app-01".into()),
        })
        .await
        .unwrap();
    Fixture {
        _root: root,
        repository: OperationRestoreRepository::new(database.pool().clone()),
        database,
        run_id: run.id,
        server_id: server.id,
    }
}

fn point(fixture: &Fixture) -> NewOperationRestorePoint {
    NewOperationRestorePoint {
        operation_run_id: fixture.run_id,
        server_id: fixture.server_id.clone(),
        task_id: "system.hostname.set".into(),
        local_relative_dir: format!("backups/tasks/{}", fixture.run_id),
        remote_asset_id: None,
        expires_at: None,
    }
}

fn item(
    restore_point_id: Uuid,
    run_id: Uuid,
    ordinal: usize,
    kind: BackupItemKind,
) -> NewOperationRestoreItem {
    NewOperationRestoreItem {
        restore_point_id,
        ordinal,
        item_kind: kind,
        remote_target: format!("/etc/qingzhou-{ordinal}.conf"),
        local_relative_path: Some(format!("backups/tasks/{run_id}/{ordinal}.bin")),
        sha256: Some("a".repeat(64)),
        original_metadata: json!({"mode": "0644", "owner": "root"}),
        status: OperationRestoreItemStatus::Available,
        error_summary: None,
    }
}

#[tokio::test]
async fn restore_point_and_items_survive_database_reopen() {
    let fixture = fixture().await;
    let created = fixture.repository.create(point(&fixture)).await.unwrap();
    assert_eq!(created.status, OperationRestorePointStatus::Creating);
    fixture
        .repository
        .add_item(item(
            created.id,
            fixture.run_id,
            0,
            BackupItemKind::RemoteFile,
        ))
        .await
        .unwrap();
    fixture
        .repository
        .add_item(item(
            created.id,
            fixture.run_id,
            1,
            BackupItemKind::CommandSnapshot,
        ))
        .await
        .unwrap();
    fixture.repository.mark_available(created.id).await.unwrap();

    let reopened = Database::open(fixture._root.path()).await.unwrap();
    let repository = OperationRestoreRepository::new(reopened.pool().clone());
    let details = repository.get(created.id).await.unwrap().unwrap();
    assert_eq!(details.point.status, OperationRestorePointStatus::Available);
    assert_eq!(details.items.len(), 2);
    assert_eq!(details.items[0].ordinal, 0);
    assert_eq!(details.items[1].item_kind, BackupItemKind::CommandSnapshot);
    let public_json = serde_json::to_string(&details).unwrap();
    assert!(!public_json.contains("remoteTarget"));
    assert!(!public_json.contains("/etc/qingzhou-"));

    let by_run = repository.list_by_run(fixture.run_id).await.unwrap();
    assert_eq!(by_run.len(), 1);
    assert_eq!(by_run[0].point.id, created.id);
}

#[tokio::test]
async fn consumed_restore_point_cannot_be_rolled_back_twice() {
    let fixture = fixture().await;
    let created = fixture.repository.create(point(&fixture)).await.unwrap();
    let mut partial = item(created.id, fixture.run_id, 0, BackupItemKind::RemoteFile);
    partial.local_relative_path = Some(format!(
        "backups/tasks/{}/unfinished.partial",
        fixture.run_id
    ));
    assert!(fixture.repository.add_item(partial).await.is_err());
    fixture
        .repository
        .add_item(item(
            created.id,
            fixture.run_id,
            0,
            BackupItemKind::RuntimeState,
        ))
        .await
        .unwrap();
    fixture.repository.mark_available(created.id).await.unwrap();
    fixture.repository.begin_rollback(created.id).await.unwrap();
    fixture
        .repository
        .finish_rollback(created.id, OperationRestorePointStatus::RolledBack, None)
        .await
        .unwrap();

    let error = fixture
        .repository
        .begin_rollback(created.id)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "restore_point_already_consumed");
}

#[tokio::test]
async fn rollback_claim_is_atomic_and_only_one_caller_wins() {
    let fixture = fixture().await;
    let created = fixture.repository.create(point(&fixture)).await.unwrap();
    fixture
        .repository
        .add_item(item(
            created.id,
            fixture.run_id,
            0,
            BackupItemKind::ManagedBlock,
        ))
        .await
        .unwrap();
    fixture.repository.mark_available(created.id).await.unwrap();

    let left = fixture.repository.clone();
    let right = fixture.repository.clone();
    let (left, right) = tokio::join!(
        left.begin_rollback(created.id),
        right.begin_rollback(created.id)
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
}

#[tokio::test]
async fn local_paths_are_relative_and_operation_deletion_is_restricted() {
    let fixture = fixture().await;
    let mut invalid = point(&fixture);
    invalid.local_relative_dir = r#"C:\escape"#.into();
    assert!(fixture.repository.create(invalid).await.is_err());

    let created = fixture.repository.create(point(&fixture)).await.unwrap();
    fixture
        .repository
        .add_item(item(
            created.id,
            fixture.run_id,
            0,
            BackupItemKind::RemoteFile,
        ))
        .await
        .unwrap();
    fixture.repository.mark_available(created.id).await.unwrap();

    let deletion = sqlx::query("DELETE FROM operation_runs WHERE id=?")
        .bind(fixture.run_id.to_string())
        .execute(fixture.database.pool())
        .await;
    assert!(deletion.is_err());

    let bad_status =
        sqlx::query("UPDATE operation_restore_points SET status='invented' WHERE id=?")
            .bind(created.id.to_string())
            .execute(fixture.database.pool())
            .await;
    assert!(bad_status.is_err());

    let operation = OperationRepository::new(fixture.database.pool().clone())
        .get(fixture.run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(operation.run.status, OperationStatus::Validating);
}
