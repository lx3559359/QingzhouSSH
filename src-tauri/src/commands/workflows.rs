use tauri::{ipc::Channel, State};
use uuid::Uuid;

use crate::{
    core::workflows::{
        require_valid_workflow, validate_workflow as validate_workflow_draft,
        WorkflowValidationReport,
    },
    domain::{
        execution::ExecutionFile,
        workflow::{
            WorkflowDefinition, WorkflowDraft, WorkflowRunDetails, WorkflowRunFilter,
            WorkflowRunRecord, WorkflowSummary,
        },
        workflow_events::WorkflowEvent,
    },
    error::AppResult,
    services::workflow_service::StartWorkflowRunRequest,
    state::AppState,
};

use super::{services, WorkflowChannelEventSink};

#[tauri::command]
pub async fn list_workflows(state: State<'_, AppState>) -> AppResult<Vec<WorkflowSummary>> {
    services(&state).await?.workflow_repository().list().await
}

#[tauri::command]
pub async fn get_workflow(
    workflow_id: Uuid,
    version: Option<i32>,
    state: State<'_, AppState>,
) -> AppResult<Option<WorkflowDefinition>> {
    services(&state)
        .await?
        .workflow_repository()
        .get(workflow_id, version)
        .await
}

#[tauri::command]
pub async fn save_workflow(
    draft: WorkflowDraft,
    state: State<'_, AppState>,
) -> AppResult<WorkflowDefinition> {
    require_valid_workflow(&draft)?;
    services(&state)
        .await?
        .workflow_repository()
        .save(draft)
        .await
}

#[tauri::command]
pub async fn delete_workflow(workflow_id: Uuid, state: State<'_, AppState>) -> AppResult<bool> {
    services(&state)
        .await?
        .workflow_repository()
        .delete(workflow_id)
        .await
}

#[tauri::command]
pub fn validate_workflow(draft: WorkflowDraft) -> WorkflowValidationReport {
    validate_workflow_draft(&draft)
}

#[tauri::command]
pub async fn start_workflow_run(
    request: StartWorkflowRunRequest,
    on_event: Channel<WorkflowEvent>,
    state: State<'_, AppState>,
) -> AppResult<WorkflowRunDetails> {
    let mut events = WorkflowChannelEventSink(on_event);
    services(&state)
        .await?
        .workflow_service()
        .run(request, &mut events)
        .await
}

#[tauri::command]
pub async fn cancel_workflow_run(run_id: Uuid, state: State<'_, AppState>) -> AppResult<()> {
    services(&state)
        .await?
        .workflow_service()
        .cancel(run_id)
        .await
}

#[tauri::command]
pub async fn retry_workflow_node(
    run_id: Uuid,
    dangerous_confirmed: bool,
    on_event: Channel<WorkflowEvent>,
    state: State<'_, AppState>,
) -> AppResult<WorkflowRunDetails> {
    let mut events = WorkflowChannelEventSink(on_event);
    services(&state)
        .await?
        .workflow_service()
        .retry_failed_node(run_id, dangerous_confirmed, &mut events)
        .await
}

#[tauri::command]
pub async fn list_workflow_runs(
    filter: WorkflowRunFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<WorkflowRunRecord>> {
    services(&state)
        .await?
        .workflow_repository()
        .list_runs(filter)
        .await
}

#[tauri::command]
pub async fn get_workflow_run(
    run_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<Option<WorkflowRunDetails>> {
    services(&state)
        .await?
        .workflow_repository()
        .get_run(run_id)
        .await
}

#[tauri::command]
pub async fn rollback_workflow_run(
    run_id: Uuid,
    dangerous_confirmed: bool,
    state: State<'_, AppState>,
) -> AppResult<WorkflowRunDetails> {
    services(&state)
        .await?
        .restore_point_service()
        .rollback_run(run_id, dangerous_confirmed)
        .await
}

#[tauri::command]
pub async fn cleanup_workflow_restore_points(
    run_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<u64> {
    services(&state)
        .await?
        .restore_point_service()
        .cleanup_run(run_id)
        .await
}

#[tauri::command]
pub async fn export_workflow_diagnostics(
    run_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<ExecutionFile> {
    services(&state)
        .await?
        .workflow_diagnostics_service()
        .export(run_id)
        .await
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::domain::{
        workflow::{
            EqualityOperator, NodePosition, WorkflowCondition, WorkflowNode, WorkflowNodeConfig,
            WorkflowRunStatus,
        },
        workflow_events::WorkflowEventPayload,
    };

    #[test]
    fn workflow_contracts_use_camel_case_tags_and_snake_case_statuses() {
        let source_node_id = Uuid::new_v4();
        let condition = WorkflowNode {
            id: Uuid::new_v4(),
            name: "condition".into(),
            position: NodePosition { x: 12.0, y: 34.0 },
            config: WorkflowNodeConfig::Condition {
                source_node_id,
                predicate: WorkflowCondition::ResultField {
                    path: "service.active".into(),
                    operator: EqualityOperator::Equal,
                    value: json!(true),
                },
            },
        };
        let encoded = serde_json::to_value(condition).unwrap();
        assert_eq!(encoded["config"]["type"], "condition");
        assert_eq!(
            encoded["config"]["sourceNodeId"],
            source_node_id.to_string()
        );
        assert_eq!(encoded["config"]["predicate"]["kind"], "resultField");
        assert_eq!(
            serde_json::to_value(WorkflowRunStatus::RollbackFailed).unwrap(),
            "rollback_failed"
        );

        let request = serde_json::to_value(StartWorkflowRunRequest {
            workflow_id: Uuid::new_v4(),
            workflow_version: Some(3),
            server_id: "server-1".into(),
            dangerous_confirmed: true,
        })
        .unwrap();
        assert_eq!(request["workflowVersion"], 3);
        assert_eq!(request["dangerousConfirmed"], true);

        let event = serde_json::to_value(WorkflowEventPayload::ConditionEvaluated {
            run_id: Uuid::new_v4(),
            node_id: Uuid::new_v4(),
            result: false,
        })
        .unwrap();
        assert_eq!(event["type"], "conditionEvaluated");

        let task = serde_json::to_value(WorkflowNodeConfig::Task {
            task_id: "system.overview".into(),
            task_version: 1,
            parameters: BTreeMap::new(),
        })
        .unwrap();
        assert_eq!(task["type"], "task");
        assert_eq!(task["taskId"], "system.overview");
    }
}
