use std::path::PathBuf;

use serde_json::json;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    core::{sftp::sha256_local_file, workflows::node_kind},
    domain::{
        execution::{now_millis, ExecutionFile},
        workflow::WorkflowRunDetails,
    },
    error::{AppError, AppResult},
    repositories::workflow_repository::WorkflowRepository,
    services::server_connector::ServerConnector,
};

#[derive(Clone)]
pub struct WorkflowDiagnosticsService {
    data_root: PathBuf,
    workflows: WorkflowRepository,
    connector: ServerConnector,
}

impl WorkflowDiagnosticsService {
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

    pub async fn export(&self, run_id: Uuid) -> AppResult<ExecutionFile> {
        let details = self.require_run(run_id).await?;
        let definition = self
            .workflows
            .get(details.run.workflow_id, Some(details.run.workflow_version))
            .await?
            .ok_or_else(|| AppError::Validation("工作流版本不存在".into()))?;
        let redactor = self
            .connector
            .redactor_for_server(&details.run.server_id)
            .await?;
        let nodes = definition
            .nodes
            .iter()
            .map(|node| {
                json!({
                    "id": node.id,
                    "name": node.name,
                    "kind": node_kind(&node.config)
                })
            })
            .collect::<Vec<_>>();
        let restore_points = details
            .restore_points
            .iter()
            .map(|point| {
                json!({
                    "id": point.id,
                    "nodeId": point.node_id,
                    "remotePath": point.remote_path,
                    "originalExisted": point.original_existed,
                    "sizeBytes": point.size_bytes,
                    "checksumSha256": point.sha256,
                    "status": point.status,
                    "errorMessage": point.error_message
                })
            })
            .collect::<Vec<_>>();
        let errors = details
            .node_runs
            .iter()
            .filter_map(|node| {
                node.error_message.as_ref().map(|message| {
                    json!({
                        "nodeId": node.node_id,
                        "attempt": node.attempt,
                        "message": message
                    })
                })
            })
            .collect::<Vec<_>>();
        let bundle = json!({
            "schemaVersion": 1,
            "generatedAt": now_millis(),
            "workflow": {
                "id": definition.id,
                "name": definition.name,
                "version": definition.version,
                "checksumSha256": definition.checksum_sha256,
                "nodes": nodes
            },
            "run": details.run,
            "nodeRuns": details.node_runs,
            "timeline": details.events,
            "restorePoints": restore_points,
            "errors": errors
        });
        let bundle = redactor.redact_json(&bundle);
        let bytes = serde_json::to_vec_pretty(&bundle)
            .map_err(|_| AppError::Serialization("诊断包无法序列化".into()))?;
        let downloads = self.data_root.join("downloads");
        tokio::fs::create_dir_all(&downloads).await?;
        let file_id = Uuid::new_v4();
        let file_name = format!("workflow-diagnostics-{run_id}-{file_id}.json");
        let destination = downloads.join(&file_name);
        let temporary = downloads.join(format!(".{file_name}.partial"));
        let write_result = async {
            let mut file = tokio::fs::File::create(&temporary).await?;
            file.write_all(&bytes).await?;
            file.flush().await?;
            file.sync_all().await?;
            tokio::fs::rename(&temporary, &destination).await?;
            AppResult::Ok(())
        }
        .await;
        if let Err(error) = write_result {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error);
        }
        Ok(ExecutionFile {
            id: file_id,
            relative_path: format!("downloads/{file_name}"),
            purpose: "workflow_diagnostics".into(),
            size_bytes: u64::try_from(bytes.len())
                .map_err(|_| AppError::Validation("诊断包大小超出范围".into()))?,
            sha256: sha256_local_file(&destination).await?,
        })
    }

    async fn require_run(&self, run_id: Uuid) -> AppResult<WorkflowRunDetails> {
        self.workflows
            .get_run(run_id)
            .await?
            .ok_or_else(|| AppError::Validation("工作流运行不存在".into()))
    }
}
