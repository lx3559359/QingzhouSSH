use serde_json::{Map, Value};

use crate::{
    core::{ssh::executor::EventSink, tasks::built_in_catalog},
    domain::{
        events::{ExecutionEvent, ExecutionEventPayload},
        execution::{ExecutionDetails, ExecutionStatus},
        workflow::{WorkflowCustomMode, WorkflowNodeConfig, WorkflowNodeStatus},
    },
    error::{AppError, AppResult},
    services::{
        execution_service::{
            CustomExecutionMode, CustomExecutionRequest, ExecutionService, TaskExecutionRequest,
        },
        workflow_nodes::NodeOutcome,
    },
};

#[derive(Clone)]
pub struct ExecutionNodeAdapter {
    service: ExecutionService,
}

impl ExecutionNodeAdapter {
    pub fn new(service: ExecutionService) -> Self {
        Self { service }
    }

    pub async fn execute<E: EventSink>(
        &self,
        server_id: &str,
        config: &WorkflowNodeConfig,
        dangerous_confirmed: bool,
        events: &mut E,
    ) -> AppResult<NodeOutcome> {
        let mut capture = ResultCapture::new(events);
        let details = match config {
            WorkflowNodeConfig::Task {
                task_id,
                task_version,
                parameters,
            } => {
                let definition = built_in_catalog()
                    .into_iter()
                    .find(|definition| definition.id == *task_id)
                    .ok_or_else(|| AppError::Validation("工作流任务不存在".into()))?;
                if definition.version != *task_version {
                    return Err(AppError::Validation("工作流任务版本不存在".into()));
                }
                self.service
                    .execute_task(
                        server_id,
                        TaskExecutionRequest {
                            task_id: task_id.clone(),
                            parameters: Value::Object(Map::from_iter(parameters.clone())),
                            dangerous_confirmed,
                        },
                        &mut capture,
                    )
                    .await?
            }
            WorkflowNodeConfig::Custom {
                mode,
                content,
                timeout_seconds,
            } => {
                self.service
                    .execute_custom(
                        server_id,
                        CustomExecutionRequest {
                            mode: match mode {
                                WorkflowCustomMode::Command => CustomExecutionMode::Command,
                                WorkflowCustomMode::Script => CustomExecutionMode::Script,
                            },
                            content: content.clone(),
                            timeout_seconds: *timeout_seconds,
                            dangerous_confirmed,
                        },
                        &mut capture,
                    )
                    .await?
            }
            _ => {
                return Err(AppError::Validation(
                    "该节点不能由命令执行适配器处理".into(),
                ));
            }
        };
        map_details(details, capture.result)
    }
}

fn map_details(details: ExecutionDetails, result: Option<Value>) -> AppResult<NodeOutcome> {
    let status = match details.record.status {
        ExecutionStatus::Succeeded => WorkflowNodeStatus::Succeeded,
        ExecutionStatus::Failed => WorkflowNodeStatus::Failed,
        ExecutionStatus::Cancelled => WorkflowNodeStatus::Cancelled,
        ExecutionStatus::Uncertain => WorkflowNodeStatus::Uncertain,
        ExecutionStatus::Queued | ExecutionStatus::Running => {
            return Err(AppError::RemoteStateUncertain(
                "M2 子执行返回时仍未进入终态".into(),
            ));
        }
    };
    Ok(NodeOutcome {
        execution_id: details.record.id,
        task_id: details.record.task_id,
        status,
        exit_code: details.record.exit_code,
        result,
        output_summary: details.record.output_summary,
        error_category: details.record.error_category,
        error_message: details.record.error_message,
        retryable: details.record.retryable,
        files: details.files,
    })
}

struct ResultCapture<'a, E: EventSink> {
    inner: &'a mut E,
    result: Option<Value>,
}

impl<'a, E: EventSink> ResultCapture<'a, E> {
    fn new(inner: &'a mut E) -> Self {
        Self {
            inner,
            result: None,
        }
    }
}

impl<E: EventSink> EventSink for ResultCapture<'_, E> {
    fn send(&mut self, event: ExecutionEvent) -> AppResult<()> {
        if let ExecutionEventPayload::Finished { result, .. } = &event.payload {
            self.result = result.clone();
        }
        self.inner.send(event)
    }
}
