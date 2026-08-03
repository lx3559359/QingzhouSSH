use crate::{
    domain::workflow::{WorkflowNodeStatus, WorkflowRunStatus},
    error::{AppError, AppResult},
};

pub fn validate_run_transition(from: WorkflowRunStatus, to: WorkflowRunStatus) -> AppResult<()> {
    let allowed = matches!(
        (from, to),
        (WorkflowRunStatus::Queued, WorkflowRunStatus::Running)
            | (WorkflowRunStatus::Queued, WorkflowRunStatus::Cancelled)
            | (WorkflowRunStatus::Running, WorkflowRunStatus::Paused)
            | (WorkflowRunStatus::Running, WorkflowRunStatus::Succeeded)
            | (WorkflowRunStatus::Running, WorkflowRunStatus::Cancelled)
            | (WorkflowRunStatus::Running, WorkflowRunStatus::Uncertain)
            | (WorkflowRunStatus::Paused, WorkflowRunStatus::Running)
            | (WorkflowRunStatus::Paused, WorkflowRunStatus::Cancelled)
            | (WorkflowRunStatus::Paused, WorkflowRunStatus::RolledBack)
            | (WorkflowRunStatus::Paused, WorkflowRunStatus::RollbackFailed)
            | (WorkflowRunStatus::Succeeded, WorkflowRunStatus::RolledBack)
            | (
                WorkflowRunStatus::Succeeded,
                WorkflowRunStatus::RollbackFailed
            )
            | (WorkflowRunStatus::Cancelled, WorkflowRunStatus::RolledBack)
            | (
                WorkflowRunStatus::Cancelled,
                WorkflowRunStatus::RollbackFailed
            )
            | (WorkflowRunStatus::Uncertain, WorkflowRunStatus::RolledBack)
            | (
                WorkflowRunStatus::Uncertain,
                WorkflowRunStatus::RollbackFailed
            )
            | (
                WorkflowRunStatus::RollbackFailed,
                WorkflowRunStatus::RolledBack
            )
    );
    if allowed {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "工作流状态不能从 {} 转为 {}",
            from.as_str(),
            to.as_str()
        )))
    }
}

pub fn validate_node_transition(from: WorkflowNodeStatus, to: WorkflowNodeStatus) -> AppResult<()> {
    let allowed = matches!(
        (from, to),
        (WorkflowNodeStatus::Pending, WorkflowNodeStatus::Running)
            | (WorkflowNodeStatus::Pending, WorkflowNodeStatus::Skipped)
            | (WorkflowNodeStatus::Pending, WorkflowNodeStatus::Cancelled)
            | (WorkflowNodeStatus::Running, WorkflowNodeStatus::Succeeded)
            | (WorkflowNodeStatus::Running, WorkflowNodeStatus::Failed)
            | (WorkflowNodeStatus::Running, WorkflowNodeStatus::Cancelled)
            | (WorkflowNodeStatus::Running, WorkflowNodeStatus::Uncertain)
    );
    if allowed {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "工作流节点状态不能从 {} 转为 {}",
            from.as_str(),
            to.as_str()
        )))
    }
}
