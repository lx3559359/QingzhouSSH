use serde_json::{Map, Value};

use crate::{
    core::{ssh::executor::EventSink, tasks::built_in_catalog},
    domain::workflow::{WorkflowCustomMode, WorkflowNodeConfig},
    error::{AppError, AppResult},
    services::{
        execution_service::{
            CustomExecutionMode, CustomExecutionRequest, ExecutionService, TaskExecutionRequest,
        },
        workflow_nodes::{map_execution_details, NodeOutcome, ResultCapture},
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

    pub async fn cancel(&self, execution_id: uuid::Uuid) -> AppResult<()> {
        self.service.cancel(execution_id).await
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
        map_execution_details(details, capture.result())
    }
}
