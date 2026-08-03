pub mod execution;
pub mod io;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    core::ssh::executor::EventSink,
    domain::{
        events::{ExecutionEvent, ExecutionEventPayload},
        execution::{ExecutionDetails, ExecutionFile, ExecutionStatus},
        workflow::WorkflowNodeStatus,
    },
    error::{AppError, AppResult},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeOutcome {
    pub execution_id: Uuid,
    pub task_id: String,
    pub status: WorkflowNodeStatus,
    pub exit_code: Option<i32>,
    pub result: Option<Value>,
    pub output_summary: Option<String>,
    pub error_category: Option<String>,
    pub error_message: Option<String>,
    pub retryable: bool,
    pub files: Vec<ExecutionFile>,
}

pub(crate) fn map_execution_details(
    details: ExecutionDetails,
    result: Option<Value>,
) -> AppResult<NodeOutcome> {
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

pub(crate) struct ResultCapture<'a, E: EventSink> {
    inner: &'a mut E,
    result: Option<Value>,
}

impl<'a, E: EventSink> ResultCapture<'a, E> {
    pub(crate) fn new(inner: &'a mut E) -> Self {
        Self {
            inner,
            result: None,
        }
    }

    pub(crate) fn result(&self) -> Option<Value> {
        self.result.clone()
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
