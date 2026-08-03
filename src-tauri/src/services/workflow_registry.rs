use std::{collections::HashMap, sync::Arc};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
struct ActiveWorkflowRun {
    token: CancellationToken,
    child_execution_id: Option<Uuid>,
}

#[derive(Clone, Default)]
pub struct WorkflowRunRegistry {
    active: Arc<Mutex<HashMap<Uuid, ActiveWorkflowRun>>>,
}

impl WorkflowRunRegistry {
    pub async fn register(&self, run_id: Uuid) -> AppResult<CancellationToken> {
        let mut active = self.active.lock().await;
        if active.contains_key(&run_id) {
            return Err(AppError::Validation("工作流运行已经注册".into()));
        }
        let token = CancellationToken::new();
        active.insert(
            run_id,
            ActiveWorkflowRun {
                token: token.clone(),
                child_execution_id: None,
            },
        );
        Ok(token)
    }

    pub async fn set_child(&self, run_id: Uuid, execution_id: Uuid) -> AppResult<()> {
        let mut active = self.active.lock().await;
        let run = active
            .get_mut(&run_id)
            .ok_or_else(|| AppError::Validation("工作流运行不存在或已经结束".into()))?;
        if run.child_execution_id.is_some() {
            return Err(AppError::Validation("工作流已有正在运行的子执行".into()));
        }
        run.child_execution_id = Some(execution_id);
        Ok(())
    }

    pub async fn clear_child(&self, run_id: Uuid, execution_id: Uuid) -> AppResult<()> {
        let mut active = self.active.lock().await;
        let run = active
            .get_mut(&run_id)
            .ok_or_else(|| AppError::Validation("工作流运行不存在或已经结束".into()))?;
        if run.child_execution_id != Some(execution_id) {
            return Err(AppError::Validation("工作流子执行标识不匹配".into()));
        }
        run.child_execution_id = None;
        Ok(())
    }

    pub async fn current_child(&self, run_id: Uuid) -> Option<Uuid> {
        self.active
            .lock()
            .await
            .get(&run_id)
            .and_then(|run| run.child_execution_id)
    }

    pub async fn cancel(&self, run_id: Uuid) -> AppResult<Option<Uuid>> {
        let active = self.active.lock().await;
        let run = active
            .get(&run_id)
            .ok_or_else(|| AppError::Validation("工作流运行不存在或已经结束".into()))?;
        run.token.cancel();
        Ok(run.child_execution_id)
    }

    pub async fn remove(&self, run_id: Uuid) {
        self.active.lock().await.remove(&run_id);
    }

    pub async fn contains(&self, run_id: Uuid) -> bool {
        self.active.lock().await.contains_key(&run_id)
    }
}
