use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    core::tasks::RiskLevel,
    error::{AppError, AppResult},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Validating,
    Preflighting,
    PreviewReady,
    WaitingConfirmation,
    BackingUp,
    Running,
    Verifying,
    Succeeded,
    Failed,
    Cancelled,
    Uncertain,
    RollbackAvailable,
    RollingBack,
    RolledBack,
    RollbackPartial,
    RollbackFailed,
}

impl OperationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Validating => "validating",
            Self::Preflighting => "preflighting",
            Self::PreviewReady => "preview_ready",
            Self::WaitingConfirmation => "waiting_confirmation",
            Self::BackingUp => "backing_up",
            Self::Running => "running",
            Self::Verifying => "verifying",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Uncertain => "uncertain",
            Self::RollbackAvailable => "rollback_available",
            Self::RollingBack => "rolling_back",
            Self::RolledBack => "rolled_back",
            Self::RollbackPartial => "rollback_partial",
            Self::RollbackFailed => "rollback_failed",
        }
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Validating, Self::Preflighting)
                | (Self::Validating, Self::Failed)
                | (Self::Validating, Self::Cancelled)
                | (Self::Preflighting, Self::PreviewReady)
                | (Self::Preflighting, Self::BackingUp)
                | (Self::Preflighting, Self::Running)
                | (Self::Preflighting, Self::Failed)
                | (Self::Preflighting, Self::Cancelled)
                | (Self::Preflighting, Self::Uncertain)
                | (Self::PreviewReady, Self::WaitingConfirmation)
                | (Self::PreviewReady, Self::BackingUp)
                | (Self::PreviewReady, Self::Running)
                | (Self::PreviewReady, Self::Cancelled)
                | (Self::WaitingConfirmation, Self::BackingUp)
                | (Self::WaitingConfirmation, Self::Running)
                | (Self::WaitingConfirmation, Self::Cancelled)
                | (Self::BackingUp, Self::Running)
                | (Self::BackingUp, Self::Failed)
                | (Self::BackingUp, Self::Cancelled)
                | (Self::BackingUp, Self::Uncertain)
                | (Self::BackingUp, Self::RollbackAvailable)
                | (Self::Running, Self::Verifying)
                | (Self::Running, Self::Succeeded)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::Cancelled)
                | (Self::Running, Self::Uncertain)
                | (Self::Running, Self::RollbackAvailable)
                | (Self::Verifying, Self::Succeeded)
                | (Self::Verifying, Self::Failed)
                | (Self::Verifying, Self::Cancelled)
                | (Self::Verifying, Self::Uncertain)
                | (Self::Verifying, Self::RollbackAvailable)
                | (Self::Uncertain, Self::Succeeded)
                | (Self::Uncertain, Self::RolledBack)
                | (Self::Uncertain, Self::RollbackAvailable)
                | (Self::Failed, Self::RollbackAvailable)
                | (Self::Succeeded, Self::RollbackAvailable)
                | (Self::RollbackPartial, Self::RollbackAvailable)
                | (Self::RollbackFailed, Self::RollbackAvailable)
                | (Self::RollbackAvailable, Self::RollingBack)
                | (Self::RollingBack, Self::RolledBack)
                | (Self::RollingBack, Self::RollbackPartial)
                | (Self::RollingBack, Self::RollbackFailed)
                | (Self::RollingBack, Self::Uncertain)
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded
                | Self::Failed
                | Self::Cancelled
                | Self::Uncertain
                | Self::RolledBack
                | Self::RollbackPartial
                | Self::RollbackFailed
        )
    }
}

impl TryFrom<&str> for OperationStatus {
    type Error = AppError;

    fn try_from(value: &str) -> AppResult<Self> {
        match value {
            "validating" => Ok(Self::Validating),
            "preflighting" => Ok(Self::Preflighting),
            "preview_ready" => Ok(Self::PreviewReady),
            "waiting_confirmation" => Ok(Self::WaitingConfirmation),
            "backing_up" => Ok(Self::BackingUp),
            "running" => Ok(Self::Running),
            "verifying" => Ok(Self::Verifying),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "uncertain" => Ok(Self::Uncertain),
            "rollback_available" => Ok(Self::RollbackAvailable),
            "rolling_back" => Ok(Self::RollingBack),
            "rolled_back" => Ok(Self::RolledBack),
            "rollback_partial" => Ok(Self::RollbackPartial),
            "rollback_failed" => Ok(Self::RollbackFailed),
            other => Err(AppError::Validation(format!("未知运维状态：{other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationPhase {
    Preflight,
    Backup,
    Execute,
    Verify,
    Rollback,
}

impl OperationPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preflight => "preflight",
            Self::Backup => "backup",
            Self::Execute => "execute",
            Self::Verify => "verify",
            Self::Rollback => "rollback",
        }
    }
}

impl TryFrom<&str> for OperationPhase {
    type Error = AppError;

    fn try_from(value: &str) -> AppResult<Self> {
        match value {
            "preflight" => Ok(Self::Preflight),
            "backup" => Ok(Self::Backup),
            "execute" => Ok(Self::Execute),
            "verify" => Ok(Self::Verify),
            "rollback" => Ok(Self::Rollback),
            other => Err(AppError::Validation(format!("未知运维阶段：{other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStepStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Uncertain,
    Skipped,
}

impl OperationStepStatus {
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

    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Pending, Self::Running)
                | (Self::Pending, Self::Cancelled)
                | (Self::Pending, Self::Skipped)
                | (Self::Running, Self::Succeeded)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::Cancelled)
                | (Self::Running, Self::Uncertain)
                | (Self::Failed, Self::Running)
                | (Self::Uncertain, Self::Running)
                | (Self::Uncertain, Self::Succeeded)
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Uncertain | Self::Skipped
        )
    }
}

impl TryFrom<&str> for OperationStepStatus {
    type Error = AppError;

    fn try_from(value: &str) -> AppResult<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "uncertain" => Ok(Self::Uncertain),
            "skipped" => Ok(Self::Skipped),
            other => Err(AppError::Validation(format!("未知运维步骤状态：{other}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRunRecord {
    pub id: Uuid,
    pub server_id: String,
    pub task_id: String,
    pub task_version: i32,
    pub risk_level: RiskLevel,
    pub status: OperationStatus,
    pub parameters_summary: Option<String>,
    pub result: Option<Value>,
    pub error_category: Option<String>,
    pub error_message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationStepRecord {
    pub run_id: Uuid,
    pub phase: OperationPhase,
    pub step_index: usize,
    pub step_id: String,
    pub title: String,
    pub status: OperationStepStatus,
    pub execution_id: Option<Uuid>,
    pub output_summary: Option<String>,
    pub error_message: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationDetails {
    pub run: OperationRunRecord,
    pub steps: Vec<OperationStepRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationFilter {
    pub server_id: Option<String>,
    pub task_id: Option<String>,
    pub status: Option<OperationStatus>,
    pub created_from: Option<i64>,
    pub created_to: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct NewOperationRun {
    pub server_id: String,
    pub task_id: String,
    pub task_version: i32,
    pub risk_level: RiskLevel,
    pub parameters_summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewOperationStep {
    pub run_id: Uuid,
    pub phase: OperationPhase,
    pub step_index: usize,
    pub step_id: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct FinishOperationStep {
    pub run_id: Uuid,
    pub phase: OperationPhase,
    pub step_index: usize,
    pub status: OperationStepStatus,
    pub execution_id: Option<Uuid>,
    pub output_summary: Option<String>,
    pub error_message: Option<String>,
    pub finished_at: i64,
}
