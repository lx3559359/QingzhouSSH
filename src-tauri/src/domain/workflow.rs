use std::{collections::BTreeMap, str::FromStr};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodePosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowNode {
    pub id: Uuid,
    pub name: String,
    pub position: NodePosition,
    pub config: WorkflowNodeConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkflowNodeConfig {
    Start,
    Task {
        task_id: String,
        task_version: i32,
        parameters: BTreeMap<String, Value>,
    },
    Custom {
        mode: WorkflowCustomMode,
        content: String,
        timeout_seconds: u64,
    },
    Upload {
        local_path: String,
        remote_path: String,
        overwrite: bool,
        create_restore_point: bool,
    },
    Download {
        remote_path: String,
        suggested_name: String,
        overwrite: bool,
    },
    LogSearch {
        path: String,
        keyword: String,
        case_sensitive: bool,
        context_lines: u8,
        limit: u32,
        start_time: Option<String>,
        end_time: Option<String>,
    },
    Condition {
        source_node_id: Uuid,
        predicate: WorkflowCondition,
    },
    Stop {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowCustomMode {
    Command,
    Script,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WorkflowCondition {
    ExitCode {
        operator: NumericOperator,
        value: i32,
    },
    ResultField {
        path: String,
        operator: EqualityOperator,
        value: Value,
    },
    OutputContains {
        text: String,
        negated: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumericOperator {
    Equal,
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EqualityOperator {
    Equal,
    NotEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowEdgeBranch {
    Success,
    True,
    False,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowEdge {
    pub from: Uuid,
    pub to: Uuid,
    pub branch: WorkflowEdgeBranch,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDraft {
    pub id: Option<Uuid>,
    pub name: String,
    pub description: String,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDefinition {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub version: i32,
    pub checksum_sha256: String,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
}

impl From<WorkflowDefinition> for WorkflowDraft {
    fn from(value: WorkflowDefinition) -> Self {
        Self {
            id: Some(value.id),
            name: value.name,
            description: value.description,
            nodes: value.nodes,
            edges: value.edges,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSummary {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub current_version: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Queued,
    Running,
    Paused,
    Succeeded,
    Cancelled,
    Uncertain,
    RolledBack,
    RollbackFailed,
}

impl WorkflowRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Succeeded => "succeeded",
            Self::Cancelled => "cancelled",
            Self::Uncertain => "uncertain",
            Self::RolledBack => "rolled_back",
            Self::RollbackFailed => "rollback_failed",
        }
    }
}

impl FromStr for WorkflowRunStatus {
    type Err = AppError;

    fn from_str(value: &str) -> AppResult<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "paused" => Ok(Self::Paused),
            "succeeded" => Ok(Self::Succeeded),
            "cancelled" => Ok(Self::Cancelled),
            "uncertain" => Ok(Self::Uncertain),
            "rolled_back" => Ok(Self::RolledBack),
            "rollback_failed" => Ok(Self::RollbackFailed),
            _ => Err(AppError::Validation("数据库中的工作流状态无效".into())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Uncertain,
    Skipped,
}

impl WorkflowNodeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Uncertain => "uncertain",
            Self::Skipped => "skipped",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Uncertain | Self::Skipped
        )
    }
}

impl FromStr for WorkflowNodeStatus {
    type Err = AppError;

    fn from_str(value: &str) -> AppResult<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "uncertain" => Ok(Self::Uncertain),
            "skipped" => Ok(Self::Skipped),
            _ => Err(AppError::Validation("数据库中的工作流节点状态无效".into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunRecord {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_version: i32,
    pub server_id: String,
    pub status: WorkflowRunStatus,
    pub current_node_id: Option<Uuid>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub duration_ms: Option<u64>,
    pub error_category: Option<String>,
    pub error_message: Option<String>,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowNodeRun {
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub attempt: i32,
    pub status: WorkflowNodeStatus,
    pub execution_id: Option<Uuid>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub duration_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub output_summary: Option<String>,
    pub error_message: Option<String>,
    pub retryable: bool,
}

#[derive(Debug, Clone)]
pub struct FinishWorkflowNode {
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub attempt: i32,
    pub status: WorkflowNodeStatus,
    pub finished_at: i64,
    pub exit_code: Option<i32>,
    pub output_summary: Option<String>,
    pub error_message: Option<String>,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunEvent {
    pub run_id: Uuid,
    pub sequence: i64,
    pub event_type: String,
    pub payload: Value,
    pub emitted_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunDetails {
    pub run: WorkflowRunRecord,
    pub node_runs: Vec<WorkflowNodeRun>,
    pub events: Vec<WorkflowRunEvent>,
}

#[derive(Debug, Clone)]
pub struct NewWorkflowRun {
    pub workflow_id: Uuid,
    pub workflow_version: i32,
    pub server_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRunFilter {
    pub workflow_id: Option<Uuid>,
    pub server_id: Option<String>,
    pub status: Option<WorkflowRunStatus>,
    pub created_from: Option<i64>,
    pub created_to: Option<i64>,
}
