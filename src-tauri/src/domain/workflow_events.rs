use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    core::redaction::Redactor,
    domain::{
        events::MAX_EVENT_BYTES,
        execution::now_millis,
        workflow::{WorkflowNodeStatus, WorkflowRunStatus},
    },
    error::{AppError, AppResult},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowEvent {
    pub sequence: u64,
    pub emitted_at: i64,
    #[serde(flatten)]
    pub payload: WorkflowEventPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkflowEventPayload {
    RunStarted {
        run_id: Uuid,
        workflow_id: Uuid,
        server_id: String,
    },
    RunStatusChanged {
        run_id: Uuid,
        status: WorkflowRunStatus,
        message: Option<String>,
    },
    NodeStarted {
        run_id: Uuid,
        node_id: Uuid,
        attempt: i32,
    },
    NodeStatusChanged {
        run_id: Uuid,
        node_id: Uuid,
        attempt: i32,
        status: WorkflowNodeStatus,
        execution_id: Option<Uuid>,
        message: Option<String>,
    },
    ConditionEvaluated {
        run_id: Uuid,
        node_id: Uuid,
        result: bool,
    },
    RestorePointChanged {
        run_id: Uuid,
        node_id: Uuid,
        restore_point_id: Uuid,
        status: String,
    },
    Finished {
        run_id: Uuid,
        status: WorkflowRunStatus,
        duration_ms: u64,
    },
}

pub trait WorkflowEventSink {
    fn send(&mut self, event: WorkflowEvent) -> AppResult<()>;
}

#[derive(Debug, Default)]
pub struct VecWorkflowEventSink {
    pub events: Vec<WorkflowEvent>,
}

impl WorkflowEventSink for VecWorkflowEventSink {
    fn send(&mut self, event: WorkflowEvent) -> AppResult<()> {
        self.events.push(event);
        Ok(())
    }
}

pub struct WorkflowEventEmitter<'a, S: WorkflowEventSink> {
    sink: &'a mut S,
    redactor: Redactor,
    sequence: u64,
}

impl<'a, S: WorkflowEventSink> WorkflowEventEmitter<'a, S> {
    pub fn new(sink: &'a mut S, redactor: Redactor) -> Self {
        Self {
            sink,
            redactor,
            sequence: 0,
        }
    }

    pub fn emit(&mut self, payload: WorkflowEventPayload) -> AppResult<()> {
        let payload = serde_json::to_value(payload)
            .map_err(|_| AppError::Serialization("工作流事件无法序列化".into()))?;
        let payload = self.redactor.redact_json(&payload);
        let payload: WorkflowEventPayload = serde_json::from_value(payload)
            .map_err(|_| AppError::Serialization("脱敏后的工作流事件无效".into()))?;
        let sequence = self.sequence.saturating_add(1);
        let event = WorkflowEvent {
            sequence,
            emitted_at: now_millis(),
            payload,
        };
        let encoded = serde_json::to_vec(&event)
            .map_err(|_| AppError::Serialization("工作流事件无法编码".into()))?;
        if encoded.len() > MAX_EVENT_BYTES {
            return Err(AppError::Validation(format!(
                "工作流事件超过 {MAX_EVENT_BYTES} 字节"
            )));
        }
        self.sink.send(event)?;
        self.sequence = sequence;
        Ok(())
    }
}
