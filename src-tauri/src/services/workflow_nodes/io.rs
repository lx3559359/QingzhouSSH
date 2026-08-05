use std::path::PathBuf;

use crate::{
    core::{
        logs::{LogSearchRequest, LogSearchTarget},
        sftp::{DownloadRequest, UploadRequest},
        ssh::executor::EventSink,
    },
    domain::workflow::{WorkflowNodeConfig, WorkflowNodeStatus},
    error::{AppError, AppResult},
    services::{
        log_service::LogService,
        transfer_service::TransferService,
        workflow_nodes::{map_execution_details, NodeOutcome, ResultCapture},
    },
};

#[derive(Clone)]
pub struct IoNodeAdapter {
    logs: LogService,
    transfers: TransferService,
}

impl IoNodeAdapter {
    pub fn new(logs: LogService, transfers: TransferService) -> Self {
        Self { logs, transfers }
    }

    pub async fn execute<E: EventSink>(
        &self,
        server_id: &str,
        config: &WorkflowNodeConfig,
        events: &mut E,
    ) -> AppResult<NodeOutcome> {
        let mut capture = ResultCapture::new(events);
        let details = match config {
            WorkflowNodeConfig::LogSearch {
                path,
                keyword,
                case_sensitive,
                context_lines,
                limit,
                start_time,
                end_time,
            } => {
                self.logs
                    .search(
                        server_id,
                        LogSearchRequest {
                            target: LogSearchTarget::Content,
                            path: path.clone(),
                            keyword: keyword.clone(),
                            case_sensitive: *case_sensitive,
                            context_lines: *context_lines,
                            limit: *limit,
                            start_time: start_time.clone(),
                            end_time: end_time.clone(),
                        },
                        &mut capture,
                    )
                    .await?
            }
            WorkflowNodeConfig::Upload {
                local_path,
                remote_path,
                overwrite,
                create_restore_point,
            } => {
                if *create_restore_point {
                    return Err(AppError::Validation(
                        "恢复点上传必须由工作流运行器预处理".into(),
                    ));
                }
                self.transfers
                    .upload(
                        server_id,
                        UploadRequest {
                            local_path: PathBuf::from(local_path),
                            remote_path: remote_path.clone(),
                            overwrite: *overwrite,
                        },
                        &mut capture,
                    )
                    .await?
            }
            WorkflowNodeConfig::Download {
                remote_path,
                suggested_name,
                overwrite,
            } => {
                self.transfers
                    .download(
                        server_id,
                        DownloadRequest {
                            remote_path: remote_path.clone(),
                            suggested_name: suggested_name.clone(),
                            overwrite: *overwrite,
                        },
                        &mut capture,
                    )
                    .await?
            }
            _ => {
                return Err(AppError::Validation(
                    "该节点不能由日志和传输适配器处理".into(),
                ));
            }
        };
        let outcome = map_execution_details(details, capture.result())?;
        if outcome.status == WorkflowNodeStatus::Succeeded
            && matches!(config, WorkflowNodeConfig::Download { .. })
            && outcome.files.is_empty()
        {
            return Err(AppError::Integrity("下载节点成功但没有登记校验文件".into()));
        }
        Ok(outcome)
    }
}
