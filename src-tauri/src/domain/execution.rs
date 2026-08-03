use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Uncertain,
}

impl ExecutionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Uncertain => "uncertain",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Uncertain
        )
    }
}

impl TryFrom<&str> for ExecutionStatus {
    type Error = AppError;

    fn try_from(value: &str) -> AppResult<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "uncertain" => Ok(Self::Uncertain),
            other => Err(AppError::Validation(format!("未知执行状态：{other}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRecord {
    pub id: Uuid,
    pub server_id: String,
    pub task_id: String,
    pub task_version: i32,
    pub category: String,
    pub status: ExecutionStatus,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub duration_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub error_category: Option<String>,
    pub error_message: Option<String>,
    pub retryable: bool,
    pub parameters_summary: Option<String>,
    pub output_summary: Option<String>,
    pub remote_process_group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionParameter {
    pub name: String,
    pub display_value: String,
    pub sensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionFile {
    pub id: Uuid,
    pub relative_path: String,
    pub purpose: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct NewExecution {
    pub server_id: String,
    pub task_id: String,
    pub task_version: i32,
    pub category: String,
    pub parameters: Vec<ExecutionParameter>,
}

#[derive(Debug, Clone)]
pub struct FinishExecution {
    pub id: Uuid,
    pub status: ExecutionStatus,
    pub finished_at: i64,
    pub duration_ms: u64,
    pub exit_code: Option<i32>,
    pub error_category: Option<String>,
    pub error_message: Option<String>,
    pub retryable: bool,
    pub output_summary: Option<String>,
    pub remote_process_group: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionFilter {
    pub server_id: Option<String>,
    pub category: Option<String>,
    pub status: Option<ExecutionStatus>,
    pub created_from: Option<i64>,
    pub created_to: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionDetails {
    pub record: ExecutionRecord,
    pub parameters: Vec<ExecutionParameter>,
    pub files: Vec<ExecutionFile>,
}

pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
