pub mod execution;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::{execution::ExecutionFile, workflow::WorkflowNodeStatus};

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
