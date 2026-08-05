use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    core::tasks::BackupItemKind,
    error::{AppError, AppResult},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationRestorePointStatus {
    Creating,
    Available,
    RollingBack,
    RolledBack,
    Partial,
    Failed,
    Expired,
    CleanupPending,
}

impl OperationRestorePointStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Creating => "creating",
            Self::Available => "available",
            Self::RollingBack => "rolling_back",
            Self::RolledBack => "rolled_back",
            Self::Partial => "partial",
            Self::Failed => "failed",
            Self::Expired => "expired",
            Self::CleanupPending => "cleanup_pending",
        }
    }
}

impl TryFrom<&str> for OperationRestorePointStatus {
    type Error = AppError;

    fn try_from(value: &str) -> AppResult<Self> {
        match value {
            "creating" => Ok(Self::Creating),
            "available" => Ok(Self::Available),
            "rolling_back" => Ok(Self::RollingBack),
            "rolled_back" => Ok(Self::RolledBack),
            "partial" => Ok(Self::Partial),
            "failed" => Ok(Self::Failed),
            "expired" => Ok(Self::Expired),
            "cleanup_pending" => Ok(Self::CleanupPending),
            other => Err(AppError::Validation(format!("未知运维恢复点状态：{other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationRestoreItemStatus {
    Pending,
    Available,
    RollingBack,
    RolledBack,
    Failed,
    Skipped,
}

impl OperationRestoreItemStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Available => "available",
            Self::RollingBack => "rolling_back",
            Self::RolledBack => "rolled_back",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

impl TryFrom<&str> for OperationRestoreItemStatus {
    type Error = AppError;

    fn try_from(value: &str) -> AppResult<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "available" => Ok(Self::Available),
            "rolling_back" => Ok(Self::RollingBack),
            "rolled_back" => Ok(Self::RolledBack),
            "failed" => Ok(Self::Failed),
            "skipped" => Ok(Self::Skipped),
            other => Err(AppError::Validation(format!("未知运维恢复项状态：{other}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRestorePoint {
    pub id: Uuid,
    pub operation_run_id: Uuid,
    pub server_id: String,
    pub task_id: String,
    pub status: OperationRestorePointStatus,
    pub local_relative_dir: String,
    pub remote_asset_id: Option<String>,
    pub expires_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRestoreItem {
    pub id: Uuid,
    pub restore_point_id: Uuid,
    pub ordinal: usize,
    pub item_kind: BackupItemKind,
    pub remote_target: String,
    pub local_relative_path: Option<String>,
    pub sha256: Option<String>,
    pub original_metadata: Value,
    pub status: OperationRestoreItemStatus,
    pub error_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRestoreDetails {
    pub point: OperationRestorePoint,
    pub items: Vec<OperationRestoreItem>,
}

#[derive(Debug, Clone)]
pub struct NewOperationRestorePoint {
    pub operation_run_id: Uuid,
    pub server_id: String,
    pub task_id: String,
    pub local_relative_dir: String,
    pub remote_asset_id: Option<String>,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct NewOperationRestoreItem {
    pub restore_point_id: Uuid,
    pub ordinal: usize,
    pub item_kind: BackupItemKind,
    pub remote_target: String,
    pub local_relative_path: Option<String>,
    pub sha256: Option<String>,
    pub original_metadata: Value,
    pub status: OperationRestoreItemStatus,
    pub error_summary: Option<String>,
}
