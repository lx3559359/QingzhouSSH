use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    core::sftp::TransferPhase,
    error::{AppError, AppResult},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirection {
    Upload,
    Download,
}

impl TransferDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Download => "download",
        }
    }
}

impl TryFrom<&str> for TransferDirection {
    type Error = AppError;

    fn try_from(value: &str) -> AppResult<Self> {
        match value {
            "upload" => Ok(Self::Upload),
            "download" => Ok(Self::Download),
            other => Err(AppError::Validation(format!("未知传输方向：{other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferJobStatus {
    Queued,
    Connecting,
    Transferring,
    Verifying,
    Finalizing,
    Succeeded,
    Failed,
    Cancelled,
    Uncertain,
}

impl TransferJobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Connecting => "connecting",
            Self::Transferring => "transferring",
            Self::Verifying => "verifying",
            Self::Finalizing => "finalizing",
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

    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Connecting | Self::Transferring | Self::Verifying | Self::Finalizing
        )
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        if self == next {
            return true;
        }
        match self {
            Self::Queued => matches!(next, Self::Connecting | Self::Cancelled),
            Self::Connecting => matches!(
                next,
                Self::Transferring | Self::Failed | Self::Cancelled | Self::Uncertain
            ),
            Self::Transferring => matches!(
                next,
                Self::Verifying
                    | Self::Finalizing
                    | Self::Failed
                    | Self::Cancelled
                    | Self::Uncertain
            ),
            Self::Verifying => matches!(
                next,
                Self::Finalizing | Self::Failed | Self::Cancelled | Self::Uncertain
            ),
            Self::Finalizing => matches!(
                next,
                Self::Succeeded | Self::Failed | Self::Cancelled | Self::Uncertain
            ),
            Self::Failed | Self::Uncertain => matches!(next, Self::Queued),
            Self::Succeeded | Self::Cancelled => false,
        }
    }
}

impl TryFrom<&str> for TransferJobStatus {
    type Error = AppError;

    fn try_from(value: &str) -> AppResult<Self> {
        match value {
            "queued" => Ok(Self::Queued),
            "connecting" => Ok(Self::Connecting),
            "transferring" => Ok(Self::Transferring),
            "verifying" => Ok(Self::Verifying),
            "finalizing" => Ok(Self::Finalizing),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "uncertain" => Ok(Self::Uncertain),
            other => Err(AppError::Validation(format!("未知传输状态：{other}"))),
        }
    }
}

impl From<TransferPhase> for TransferJobStatus {
    fn from(value: TransferPhase) -> Self {
        match value {
            TransferPhase::Connecting => Self::Connecting,
            TransferPhase::Transferring => Self::Transferring,
            TransferPhase::Verifying => Self::Verifying,
            TransferPhase::Finalizing => Self::Finalizing,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferJob {
    pub id: Uuid,
    pub execution_id: Option<Uuid>,
    pub server_id: String,
    pub direction: TransferDirection,
    pub source_path: String,
    pub target_path: String,
    pub overwrite: bool,
    pub verification: String,
    pub status: TransferJobStatus,
    pub transferred: u64,
    pub total: Option<u64>,
    pub percent: Option<f64>,
    pub bytes_per_second: Option<f64>,
    pub average_bytes_per_second: Option<f64>,
    pub eta_seconds: Option<u64>,
    pub attempt_count: u32,
    pub max_attempts: u32,
    pub cancel_requested: bool,
    pub retryable: bool,
    pub error_category: Option<String>,
    pub error_message: Option<String>,
    pub sha256: Option<String>,
    pub location: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct NewTransferJob {
    pub server_id: String,
    pub direction: TransferDirection,
    pub source_path: String,
    pub target_path: String,
    pub overwrite: bool,
    pub verification: String,
}

pub fn can_automatically_retry(
    category: &str,
    message: &str,
    attempt_count: u32,
    max_attempts: u32,
) -> bool {
    if attempt_count >= max_attempts || !matches!(category, "io" | "ssh" | "transfer") {
        return false;
    }
    let message = message.to_ascii_lowercase();
    const NON_TRANSIENT_MARKERS: &[&str] = &[
        "auth",
        "host key",
        "fingerprint",
        "permission",
        "denied",
        "disk",
        "no space",
        "integrity",
        "checksum",
        "hash mismatch",
    ];
    !NON_TRANSIENT_MARKERS
        .iter()
        .any(|marker| message.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::{can_automatically_retry, TransferJobStatus};

    #[test]
    fn transfer_job_status_only_allows_safe_transitions() {
        assert!(TransferJobStatus::Queued.can_transition_to(TransferJobStatus::Connecting));
        assert!(TransferJobStatus::Queued.can_transition_to(TransferJobStatus::Cancelled));
        assert!(TransferJobStatus::Transferring.can_transition_to(TransferJobStatus::Verifying));
        assert!(TransferJobStatus::Failed.can_transition_to(TransferJobStatus::Queued));
        assert!(!TransferJobStatus::Succeeded.can_transition_to(TransferJobStatus::Queued));
        assert!(!TransferJobStatus::Cancelled.can_transition_to(TransferJobStatus::Transferring));
    }

    #[test]
    fn automatic_retry_excludes_non_transient_failures() {
        assert!(can_automatically_retry("io", "connection reset", 1, 3));
        assert!(can_automatically_retry("ssh", "connection timed out", 2, 3));
        assert!(!can_automatically_retry(
            "ssh",
            "authentication failed",
            1,
            3
        ));
        assert!(!can_automatically_retry("permission", "denied", 1, 3));
        assert!(!can_automatically_retry("disk_space", "full", 1, 3));
        assert!(!can_automatically_retry("integrity", "hash mismatch", 1, 3));
        assert!(!can_automatically_retry("io", "connection reset", 3, 3));
    }
}
