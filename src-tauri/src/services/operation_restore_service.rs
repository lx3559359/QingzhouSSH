use std::path::PathBuf;

use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    core::{
        sftp::{backup_operation_remote_file, OperationFileBackup, RemoteFileMetadata},
        ssh::transport::execute_authenticated,
        tasks::{
            render_backup_target, task_restore_dir, task_restore_item_relative_path,
            write_restore_asset_atomic, BackupItemKind, BackupPlan, ValidatedParameters,
        },
    },
    domain::operation_restore::{
        NewOperationRestoreItem, NewOperationRestorePoint, OperationRestoreDetails,
        OperationRestoreItemStatus,
    },
    error::{AppError, AppResult},
    repositories::operation_restore_repository::OperationRestoreRepository,
    services::server_connector::{ConnectedServer, ServerConnector},
};

#[derive(Clone)]
pub struct OperationRestoreService {
    data_root: PathBuf,
    repository: OperationRestoreRepository,
    connector: ServerConnector,
}

impl OperationRestoreService {
    pub fn new(
        data_root: PathBuf,
        repository: OperationRestoreRepository,
        connector: ServerConnector,
    ) -> Self {
        Self {
            data_root,
            repository,
            connector,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn capture(
        &self,
        run_id: Uuid,
        server_id: &str,
        task_id: &str,
        implementation_id: &str,
        backup_plan: &BackupPlan,
        parameters: &ValidatedParameters,
        cancel: CancellationToken,
    ) -> AppResult<OperationRestoreDetails> {
        if backup_plan.items.is_empty() {
            return Err(AppError::Validation("危险任务没有声明恢复项".into()));
        }
        let local_relative_dir = task_restore_dir(run_id)
            .to_string_lossy()
            .replace('\\', "/");
        let creating = self
            .repository
            .create(NewOperationRestorePoint {
                operation_run_id: run_id,
                server_id: server_id.into(),
                task_id: task_id.into(),
                local_relative_dir,
                remote_asset_id: None,
                expires_at: None,
            })
            .await?;
        if cancel.is_cancelled() {
            self.repository.mark_creation_failed(creating.id).await?;
            return Err(AppError::Cancelled);
        }
        let connected = match self.connector.connect(server_id).await {
            Ok(connected) => connected,
            Err(error) => {
                self.repository.mark_creation_failed(creating.id).await?;
                return Err(error);
            }
        };
        let result = self
            .capture_connected(
                creating.id,
                run_id,
                implementation_id,
                backup_plan,
                parameters,
                &connected,
                cancel,
            )
            .await;
        connected.session.disconnect().await;
        if let Err(error) = result {
            let _ = self.repository.mark_creation_failed(creating.id).await;
            return Err(error);
        }
        if let Err(error) = self.repository.mark_available(creating.id).await {
            let _ = self.repository.mark_creation_failed(creating.id).await;
            return Err(error);
        }
        self.repository
            .get(creating.id)
            .await?
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn get(&self, id: Uuid) -> AppResult<Option<OperationRestoreDetails>> {
        self.repository.get(id).await
    }

    pub async fn list_by_run(&self, run_id: Uuid) -> AppResult<Vec<OperationRestoreDetails>> {
        self.repository.list_by_run(run_id).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn capture_connected(
        &self,
        restore_point_id: Uuid,
        run_id: Uuid,
        implementation_id: &str,
        backup_plan: &BackupPlan,
        parameters: &ValidatedParameters,
        connected: &ConnectedServer,
        cancel: CancellationToken,
    ) -> AppResult<()> {
        for (ordinal, definition) in backup_plan.items.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            let target =
                render_backup_target(&definition.target_template, implementation_id, parameters)?;
            let relative = task_restore_item_relative_path(run_id, ordinal, &target)?;
            let (local_relative_path, sha256, original_metadata) = match definition.kind {
                BackupItemKind::RemoteFile | BackupItemKind::ManagedBlock => {
                    let backup: OperationFileBackup = backup_operation_remote_file(
                        &connected.session,
                        &self.data_root,
                        &target,
                        &relative,
                        cancel.child_token(),
                    )
                    .await?;
                    let metadata: Option<&RemoteFileMetadata> = backup.metadata.as_ref();
                    match backup.transfer {
                        Some(transfer) => (
                            Some(transfer.location),
                            Some(transfer.sha256),
                            json!({
                                "originalExisted": true,
                                "size": metadata.and_then(|value| value.size),
                                "uid": metadata.and_then(|value| value.uid),
                                "gid": metadata.and_then(|value| value.gid),
                                "permissions": metadata.and_then(|value| value.permissions),
                                "modifiedAt": metadata.and_then(|value| value.modified_at),
                            }),
                        ),
                        None => (None, None, json!({"originalExisted": false})),
                    }
                }
                BackupItemKind::CommandSnapshot | BackupItemKind::RuntimeState => {
                    let output = execute_authenticated(&connected.session, &target).await?;
                    if output.exit_status != 0 {
                        return Err(AppError::ssh_command(
                            output.exit_status,
                            connected.redactor.redact(&output.stderr),
                        ));
                    }
                    let snapshot = format!(
                        "stdout:\n{}\nstderr:\n{}",
                        connected.redactor.redact(&output.stdout),
                        connected.redactor.redact(&output.stderr)
                    );
                    let asset =
                        write_restore_asset_atomic(&self.data_root, &relative, snapshot.as_bytes())
                            .await?;
                    (
                        Some(asset.relative_path),
                        Some(asset.sha256),
                        json!({
                            "originalExisted": true,
                            "exitStatus": output.exit_status,
                            "bytes": asset.bytes,
                        }),
                    )
                }
            };
            self.repository
                .add_item(NewOperationRestoreItem {
                    restore_point_id,
                    ordinal,
                    item_kind: definition.kind,
                    remote_target: target,
                    local_relative_path,
                    sha256,
                    original_metadata,
                    status: OperationRestoreItemStatus::Available,
                    error_summary: None,
                })
                .await?;
        }
        Ok(())
    }
}
