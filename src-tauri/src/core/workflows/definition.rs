use serde::{Deserialize, Serialize};

use crate::domain::workflow::WorkflowNodeConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeKind {
    Start,
    Task,
    Custom,
    Upload,
    Download,
    LogSearch,
    Condition,
    Stop,
}

pub fn node_kind(config: &WorkflowNodeConfig) -> WorkflowNodeKind {
    match config {
        WorkflowNodeConfig::Start {} => WorkflowNodeKind::Start,
        WorkflowNodeConfig::Task { .. } => WorkflowNodeKind::Task,
        WorkflowNodeConfig::Custom { .. } => WorkflowNodeKind::Custom,
        WorkflowNodeConfig::Upload { .. } => WorkflowNodeKind::Upload,
        WorkflowNodeConfig::Download { .. } => WorkflowNodeKind::Download,
        WorkflowNodeConfig::LogSearch { .. } => WorkflowNodeKind::LogSearch,
        WorkflowNodeConfig::Condition { .. } => WorkflowNodeKind::Condition,
        WorkflowNodeConfig::Stop { .. } => WorkflowNodeKind::Stop,
    }
}
