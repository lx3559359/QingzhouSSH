use std::{io::ErrorKind, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    core::{redaction::Redactor, sftp::sha256_local_file},
    domain::execution::{now_millis, ExecutionFile},
    error::{AppError, AppResult},
    repositories::{
        operation_batch_repository::OperationBatchRepository,
        operation_repository::OperationRepository,
    },
    services::server_connector::ServerConnector,
};

const MAX_REPORT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportFormat {
    Json,
    Txt,
}

impl ReportFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Txt => "txt",
        }
    }
}

#[derive(Clone)]
pub struct OperationReportService {
    data_root: PathBuf,
    operations: OperationRepository,
    batches: OperationBatchRepository,
    connector: ServerConnector,
}

impl OperationReportService {
    pub fn new(
        data_root: PathBuf,
        operations: OperationRepository,
        batches: OperationBatchRepository,
        connector: ServerConnector,
    ) -> Self {
        Self {
            data_root,
            operations,
            batches,
            connector,
        }
    }

    pub async fn export_run(&self, run_id: Uuid, format: ReportFormat) -> AppResult<ExecutionFile> {
        let details = self
            .operations
            .get(run_id)
            .await?
            .ok_or_else(|| AppError::Validation("运维运行不存在".into()))?;
        if !details.run.status.is_terminal() {
            return Err(AppError::Validation("运维运行结束后才能导出报告".into()));
        }
        let payload = json!({
            "schemaVersion": 1,
            "kind": "operation",
            "generatedAt": now_millis(),
            "operation": details,
        });
        let payload = self
            .redact_for_servers(payload, std::slice::from_ref(&details.run.server_id))
            .await?;
        let bytes = encode_report(format, &payload)?;
        self.write_report(
            &format!("operation-{run_id}.{}", format.extension()),
            "operation_report",
            &bytes,
        )
        .await
    }

    pub async fn export_batch(
        &self,
        batch_id: Uuid,
        format: ReportFormat,
    ) -> AppResult<ExecutionFile> {
        let details = self
            .batches
            .get(batch_id)
            .await?
            .ok_or_else(|| AppError::Validation("批量任务不存在".into()))?;
        if !details.batch.status.is_terminal() {
            return Err(AppError::Validation("批量任务结束后才能导出报告".into()));
        }
        let mut runs = Vec::new();
        for item in &details.items {
            if let Some(run_id) = item.operation_run_id {
                if let Some(run) = self.operations.get(run_id).await? {
                    runs.push(run);
                }
            }
        }
        let server_ids = details
            .items
            .iter()
            .map(|item| item.server_id.clone())
            .collect::<Vec<_>>();
        let payload = json!({
            "schemaVersion": 1,
            "kind": "operation_batch",
            "generatedAt": now_millis(),
            "batch": details,
            "operations": runs,
        });
        let payload = self.redact_for_servers(payload, &server_ids).await?;
        let bytes = encode_report(format, &payload)?;
        self.write_report(
            &format!("batch-{batch_id}.{}", format.extension()),
            "operation_batch_report",
            &bytes,
        )
        .await
    }

    async fn redact_for_servers(
        &self,
        mut value: Value,
        server_ids: &[String],
    ) -> AppResult<Value> {
        for server_id in server_ids {
            value = self
                .connector
                .redactor_for_server(server_id)
                .await?
                .redact_json(&value);
        }
        let root = self.data_root.to_string_lossy().into_owned();
        let root_redactor = Redactor::new([root.clone(), root.replace('\\', "/")]);
        Ok(root_redactor.redact_json(&value))
    }

    async fn write_report(
        &self,
        file_name: &str,
        purpose: &str,
        bytes: &[u8],
    ) -> AppResult<ExecutionFile> {
        if bytes.len() > MAX_REPORT_BYTES {
            return Err(AppError::Validation("报告超过 16 MiB 大小上限".into()));
        }
        let directory = self.data_root.join("downloads").join("reports");
        tokio::fs::create_dir_all(&directory).await?;
        let destination = directory.join(file_name);
        if !tokio::fs::try_exists(&destination).await? {
            let temporary = directory.join(format!(".{file_name}.{}.partial", Uuid::new_v4()));
            let write_result = async {
                let mut file = tokio::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&temporary)
                    .await?;
                file.write_all(bytes).await?;
                file.flush().await?;
                file.sync_all().await?;
                drop(file);
                match tokio::fs::rename(&temporary, &destination).await {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                        tokio::fs::remove_file(&temporary).await?;
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            }
            .await;
            if let Err(error) = write_result {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(error.into());
            }
        }
        let metadata = tokio::fs::symlink_metadata(&destination).await?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(AppError::Security("报告目标不是普通文件".into()));
        }
        Ok(ExecutionFile {
            id: Uuid::new_v4(),
            relative_path: format!("downloads/reports/{file_name}"),
            purpose: purpose.into(),
            size_bytes: metadata.len(),
            sha256: sha256_local_file(&destination).await?,
        })
    }
}

fn encode_report(format: ReportFormat, payload: &Value) -> AppResult<Vec<u8>> {
    match format {
        ReportFormat::Json => serde_json::to_vec_pretty(payload)
            .map_err(|error| AppError::Serialization(error.to_string())),
        ReportFormat::Txt => Ok(render_txt(payload).into_bytes()),
    }
}

fn render_txt(payload: &Value) -> String {
    let mut output = String::from("轻舟 SSH 运维报告\n==================\n\n");
    output.push_str(&format!(
        "报告类型：{}\n生成时间：{}\n\n",
        text(payload.get("kind")),
        text(payload.get("generatedAt"))
    ));
    if let Some(operation) = payload.get("operation") {
        render_operation(&mut output, operation);
    }
    if let Some(batch) = payload.get("batch") {
        output.push_str("批量概况\n--------\n");
        if let Some(record) = batch.get("batch") {
            output.push_str(&format!(
                "任务：{}\n状态：{}\n\n",
                text(record.get("taskId")),
                text(record.get("status"))
            ));
        }
        output.push_str("服务器结果\n----------\n");
        if let Some(items) = batch.get("items").and_then(Value::as_array) {
            for item in items {
                output.push_str(&format!(
                    "- {}：{}{}\n",
                    text(item.get("serverId")),
                    text(item.get("status")),
                    optional_suffix(item.get("errorMessage"))
                ));
            }
        }
        output.push('\n');
        if let Some(operations) = payload.get("operations").and_then(Value::as_array) {
            for operation in operations {
                render_operation(&mut output, operation);
            }
        }
    }
    output
}

fn render_operation(output: &mut String, details: &Value) {
    let Some(run) = details.get("run") else {
        return;
    };
    output.push_str("任务概况\n--------\n");
    output.push_str(&format!(
        "服务器：{}\n任务：{}\n状态：{}\n\n",
        text(run.get("serverId")),
        text(run.get("taskId")),
        text(run.get("status"))
    ));
    let result = run.get("result");
    output.push_str("结论\n----\n");
    output.push_str(&format!(
        "{}\n\n",
        text(result.and_then(|value| value.get("summary")))
    ));
    output.push_str("发现\n----\n");
    if let Some(findings) = result
        .and_then(|value| value.get("findings"))
        .and_then(Value::as_array)
    {
        for finding in findings {
            output.push_str(&format!(
                "- [{}] {}：{}\n",
                text(finding.get("level")),
                text(finding.get("title")),
                text(finding.get("detail"))
            ));
        }
    }
    output.push_str("\n建议\n----\n");
    if let Some(suggestions) = result
        .and_then(|value| value.get("suggestions"))
        .and_then(Value::as_array)
    {
        for suggestion in suggestions {
            output.push_str(&format!("- {}\n", text(Some(suggestion))));
        }
    }
    output.push_str("\n技术详情（已脱敏）\n------------------\n");
    output.push_str(&text(
        result.and_then(|value| value.get("technicalDetails")),
    ));
    output.push_str("\n\n");
}

fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Null) | None => "—".into(),
        Some(value) => value.to_string(),
    }
}

fn optional_suffix(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) if !value.is_empty() => format!("（{value}）"),
        _ => String::new(),
    }
}
