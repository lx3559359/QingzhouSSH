use std::path::PathBuf;

use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    core::{
        sftp::backup_remote_file,
        workflows::{resolve_restore_point_path, restore_point_relative_path},
    },
    domain::workflow::{
        FinishWorkflowRestorePoint, NewWorkflowRestorePoint, WorkflowRestorePoint,
        WorkflowRestorePointStatus,
    },
    error::{AppError, AppResult},
    repositories::workflow_repository::WorkflowRepository,
    services::server_connector::ServerConnector,
};

#[derive(Clone)]
pub struct RestorePointService {
    data_root: PathBuf,
    workflows: WorkflowRepository,
    connector: ServerConnector,
}

impl RestorePointService {
    pub fn new(
        data_root: PathBuf,
        workflows: WorkflowRepository,
        connector: ServerConnector,
    ) -> Self {
        Self {
            data_root,
            workflows,
            connector,
        }
    }

    pub async fn capture(
        &self,
        run_id: Uuid,
        node_id: Uuid,
        server_id: &str,
        remote_path: &str,
        cancel: CancellationToken,
    ) -> AppResult<WorkflowRestorePoint> {
        let relative_path = restore_point_relative_path(run_id, node_id, remote_path)?;
        let creating = self
            .workflows
            .create_restore_point(NewWorkflowRestorePoint {
                run_id,
                node_id,
                remote_path: remote_path.into(),
                relative_path: Some(relative_path.clone()),
                applicability: json!({
                    "serverId": server_id,
                    "remotePath": remote_path,
                    "strategy": "restoreExistingOrDeleteCreated"
                }),
            })
            .await?;

        let connected = match self.connector.connect(server_id).await {
            Ok(connected) => connected,
            Err(error) => {
                self.record_failure(creating.id, &error.to_string()).await?;
                return Err(error);
            }
        };
        let outcome = backup_remote_file(
            &connected.session,
            &self.data_root,
            remote_path,
            &relative_path,
            cancel,
        )
        .await;
        connected.session.disconnect().await;

        match outcome {
            Ok(Some(outcome)) => {
                self.workflows
                    .finish_restore_point(FinishWorkflowRestorePoint {
                        id: creating.id,
                        status: WorkflowRestorePointStatus::Available,
                        original_existed: true,
                        relative_path: Some(outcome.location),
                        size_bytes: Some(outcome.bytes),
                        sha256: Some(outcome.sha256),
                        error_message: None,
                    })
                    .await?;
            }
            Ok(None) => {
                self.workflows
                    .finish_restore_point(FinishWorkflowRestorePoint {
                        id: creating.id,
                        status: WorkflowRestorePointStatus::Available,
                        original_existed: false,
                        relative_path: None,
                        size_bytes: None,
                        sha256: None,
                        error_message: None,
                    })
                    .await?;
            }
            Err(error) => {
                let message = connected.redactor.redact(&error.to_string());
                self.remove_partial(&relative_path).await;
                self.record_failure(creating.id, &message).await?;
                return Err(error);
            }
        }
        self.workflows
            .get_restore_point(creating.id)
            .await?
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    async fn record_failure(&self, id: Uuid, message: &str) -> AppResult<()> {
        self.workflows
            .finish_restore_point(FinishWorkflowRestorePoint {
                id,
                status: WorkflowRestorePointStatus::Failed,
                original_existed: false,
                relative_path: None,
                size_bytes: None,
                sha256: None,
                error_message: Some(message.into()),
            })
            .await
    }

    async fn remove_partial(&self, relative_path: &str) {
        let Ok(destination) = resolve_restore_point_path(&self.data_root, relative_path) else {
            return;
        };
        let mut partial_name = destination.file_name().unwrap_or_default().to_os_string();
        partial_name.push(".partial");
        let partial = destination.with_file_name(partial_name);
        let _ = tokio::fs::remove_file(partial).await;
    }
}
