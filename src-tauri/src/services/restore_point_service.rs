use std::path::PathBuf;

use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    core::{
        sftp::{backup_remote_file, delete_remote_file, sha256_local_file, upload, UploadRequest},
        ssh::executor::VecEventSink,
        workflows::{resolve_restore_point_path, restore_point_relative_path},
    },
    domain::workflow::{
        FinishWorkflowRestorePoint, NewWorkflowRestorePoint, WorkflowRestorePoint,
        WorkflowRestorePointStatus, WorkflowRunDetails, WorkflowRunStatus,
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

    pub async fn rollback_run(
        &self,
        run_id: Uuid,
        dangerous_confirmed: bool,
    ) -> AppResult<WorkflowRunDetails> {
        if !dangerous_confirmed {
            return Err(AppError::Validation("回滚必须二次确认".into()));
        }
        let details = self.require_run(run_id).await?;
        if !matches!(
            details.run.status,
            WorkflowRunStatus::Paused
                | WorkflowRunStatus::Succeeded
                | WorkflowRunStatus::Cancelled
                | WorkflowRunStatus::Uncertain
                | WorkflowRunStatus::RollbackFailed
        ) {
            return Err(AppError::Validation("工作流当前状态不允许回滚".into()));
        }
        let mut points = details
            .restore_points
            .iter()
            .filter(|point| point.status == WorkflowRestorePointStatus::Available)
            .cloned()
            .collect::<Vec<_>>();
        if points.is_empty() {
            return Err(AppError::Validation("工作流没有可用恢复点".into()));
        }
        points.sort_by_key(|point| (point.created_at, point.id));
        points.reverse();

        let connected = match self.connector.connect(&details.run.server_id).await {
            Ok(connected) => connected,
            Err(error) => {
                self.workflows
                    .finish_rollback_run(run_id, false, Some(error.to_string()))
                    .await?;
                return self.require_run(run_id).await;
            }
        };
        let mut failures = Vec::new();
        for point in points {
            self.workflows
                .mark_restore_point_rolling_back(point.id)
                .await?;
            let result = self
                .rollback_point(&connected.session, &details.run.server_id, &point)
                .await;
            match result {
                Ok(()) => {
                    self.workflows
                        .finish_restore_point_rollback(point.id, true, None)
                        .await?;
                }
                Err(error) => {
                    let message = connected.redactor.redact(&error.to_string());
                    self.workflows
                        .finish_restore_point_rollback(point.id, false, Some(message.clone()))
                        .await?;
                    failures.push(message);
                }
            }
        }
        connected.session.disconnect().await;
        self.workflows
            .finish_rollback_run(
                run_id,
                failures.is_empty(),
                (!failures.is_empty()).then(|| failures.join("; ")),
            )
            .await?;
        self.require_run(run_id).await
    }

    pub async fn cleanup_run(&self, run_id: Uuid) -> AppResult<u64> {
        let details = self.require_run(run_id).await?;
        if matches!(
            details.run.status,
            WorkflowRunStatus::Queued | WorkflowRunStatus::Running
        ) {
            return Err(AppError::Validation("运行中的工作流不能清理恢复点".into()));
        }
        let mut cleaned = 0_u64;
        for point in details.restore_points {
            if point.status == WorkflowRestorePointStatus::Expired {
                continue;
            }
            if let Some(relative_path) = point.relative_path.as_deref() {
                let path = resolve_restore_point_path(&self.data_root, relative_path)?;
                if path.exists() {
                    tokio::fs::remove_file(&path).await?;
                    cleaned = cleaned.saturating_add(1);
                }
            }
            self.workflows.expire_restore_point(point.id).await?;
        }
        Ok(cleaned)
    }

    async fn rollback_point(
        &self,
        session: &crate::core::ssh::transport::AuthenticatedSshSession,
        server_id: &str,
        point: &WorkflowRestorePoint,
    ) -> AppResult<()> {
        validate_applicability(point, server_id)?;
        if !point.original_existed {
            delete_remote_file(session, &point.remote_path).await?;
            return Ok(());
        }
        let relative_path = point
            .relative_path
            .as_deref()
            .ok_or_else(|| AppError::Integrity("恢复点缺少本地备份路径".into()))?;
        let expected_hash = point
            .sha256
            .as_deref()
            .ok_or_else(|| AppError::Integrity("恢复点缺少 SHA-256".into()))?;
        let local_path = resolve_restore_point_path(&self.data_root, relative_path)?;
        if !local_path.is_file() {
            return Err(AppError::Integrity("恢复点备份文件不存在".into()));
        }
        let actual_hash = sha256_local_file(&local_path).await?;
        if actual_hash != expected_hash {
            return Err(AppError::Integrity(format!(
                "恢复点 SHA-256 不一致：登记 {expected_hash}，实际 {actual_hash}"
            )));
        }
        let mut events = VecEventSink::default();
        let restored = upload(
            session,
            &UploadRequest {
                local_path,
                remote_path: point.remote_path.clone(),
                overwrite: true,
            },
            &mut events,
            CancellationToken::new(),
        )
        .await?;
        if restored.sha256 != expected_hash {
            return Err(AppError::Integrity("回滚后的远程文件校验失败".into()));
        }
        Ok(())
    }

    async fn require_run(&self, run_id: Uuid) -> AppResult<WorkflowRunDetails> {
        self.workflows
            .get_run(run_id)
            .await?
            .ok_or_else(|| AppError::Validation("工作流运行不存在".into()))
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

fn validate_applicability(point: &WorkflowRestorePoint, server_id: &str) -> AppResult<()> {
    let applicable_server = point
        .applicability
        .get("serverId")
        .and_then(serde_json::Value::as_str);
    let applicable_path = point
        .applicability
        .get("remotePath")
        .and_then(serde_json::Value::as_str);
    if applicable_server != Some(server_id) || applicable_path != Some(point.remote_path.as_str()) {
        return Err(AppError::Security(
            "恢复点不适用于当前服务器或远程路径".into(),
        ));
    }
    Ok(())
}
