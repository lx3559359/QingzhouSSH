use tauri::{ipc::Channel, State};
use uuid::Uuid;

use crate::{
    domain::{
        events::ExecutionEvent,
        execution::ExecutionFile,
        operation::{OperationDetails, OperationFilter, OperationRunRecord},
        operation_batch::{OperationBatchDetails, OperationBatchRequest},
        operation_restore::OperationRestoreDetails,
    },
    error::AppResult,
    services::{
        execution_service::TaskAvailability,
        operation_report_service::ReportFormat,
        operation_service::{
            OperationConfirmRequest, OperationPreflightRequest, OperationPreview,
            OperationRecoveryResult, OperationStartRequest,
        },
    },
    state::AppState,
};

use super::{services, ChannelEventSink};

#[tauri::command]
pub async fn list_operations_tasks(
    server_id: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<TaskAvailability>> {
    services(&state)
        .await?
        .list_task_definitions(&server_id)
        .await
}

#[tauri::command]
pub async fn preflight_operation(
    server_id: String,
    request: OperationPreflightRequest,
    state: State<'_, AppState>,
) -> AppResult<OperationPreview> {
    services(&state)
        .await?
        .operation_service()
        .preflight(&server_id, request)
        .await
}

#[tauri::command]
pub async fn preview_operation(
    server_id: String,
    request: OperationPreflightRequest,
    state: State<'_, AppState>,
) -> AppResult<OperationPreview> {
    services(&state)
        .await?
        .operation_service()
        .preflight(&server_id, request)
        .await
}

#[tauri::command]
pub async fn start_operation(
    server_id: String,
    request: OperationStartRequest,
    on_event: Channel<ExecutionEvent>,
    state: State<'_, AppState>,
) -> AppResult<OperationDetails> {
    let mut events = ChannelEventSink(on_event);
    services(&state)
        .await?
        .operation_service()
        .start(&server_id, request, &mut events)
        .await
}

#[tauri::command]
pub async fn confirm_operation(
    server_id: String,
    request: OperationConfirmRequest,
    on_event: Channel<ExecutionEvent>,
    state: State<'_, AppState>,
) -> AppResult<OperationDetails> {
    let mut events = ChannelEventSink(on_event);
    services(&state)
        .await?
        .operation_service()
        .start(&server_id, request.into(), &mut events)
        .await
}

#[tauri::command]
pub async fn list_operation_restore_points(
    run_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<Vec<OperationRestoreDetails>> {
    services(&state)
        .await?
        .operation_restore_service()
        .list_by_run(run_id)
        .await
}

#[tauri::command]
pub async fn rollback_operation(
    restore_point_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<OperationRecoveryResult> {
    services(&state)
        .await?
        .operation_service()
        .rollback_operation(restore_point_id)
        .await
}

#[tauri::command]
pub async fn inspect_uncertain_operation(
    run_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<OperationRecoveryResult> {
    services(&state)
        .await?
        .operation_service()
        .inspect_uncertain(run_id)
        .await
}

#[tauri::command]
pub async fn cleanup_operation_restore_assets(
    restore_point_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<OperationRestoreDetails> {
    services(&state)
        .await?
        .operation_restore_service()
        .cleanup_assets(restore_point_id)
        .await
}

#[tauri::command]
pub async fn cancel_operation(run_id: Uuid, state: State<'_, AppState>) -> AppResult<()> {
    services(&state)
        .await?
        .operation_service()
        .cancel(run_id)
        .await
}

#[tauri::command]
pub async fn get_operation(
    run_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<Option<OperationDetails>> {
    services(&state)
        .await?
        .operation_service()
        .get(run_id)
        .await
}

#[tauri::command]
pub async fn list_operations(
    filter: OperationFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<OperationRunRecord>> {
    services(&state)
        .await?
        .operation_service()
        .list(filter)
        .await
}

#[tauri::command]
pub async fn start_operation_batch(
    request: OperationBatchRequest,
    state: State<'_, AppState>,
) -> AppResult<OperationBatchDetails> {
    services(&state)
        .await?
        .operation_batch_service()
        .start_background(request)
        .await
}

#[tauri::command]
pub async fn cancel_operation_batch(batch_id: Uuid, state: State<'_, AppState>) -> AppResult<()> {
    services(&state)
        .await?
        .operation_batch_service()
        .cancel(batch_id)
        .await
}

#[tauri::command]
pub async fn get_operation_batch(
    batch_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<Option<OperationBatchDetails>> {
    services(&state)
        .await?
        .operation_batch_service()
        .get(batch_id)
        .await
}

#[tauri::command]
pub async fn export_operation_report(
    run_id: Uuid,
    format: ReportFormat,
    state: State<'_, AppState>,
) -> AppResult<ExecutionFile> {
    services(&state)
        .await?
        .operation_report_service()
        .export_run(run_id, format)
        .await
}

#[tauri::command]
pub async fn export_operation_batch_report(
    batch_id: Uuid,
    format: ReportFormat,
    state: State<'_, AppState>,
) -> AppResult<ExecutionFile> {
    services(&state)
        .await?
        .operation_report_service()
        .export_batch(batch_id, format)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_requests_are_camel_case_and_reject_commands() {
        let value = serde_json::to_value(OperationStartRequest {
            task_id: "system.overview".into(),
            task_version: 2,
            parameters: serde_json::json!({}),
            confirmed_preview_id: None,
        })
        .unwrap();
        assert_eq!(value["taskId"], "system.overview");
        assert!(value.get("confirmedPreviewId").is_some());
        assert!(value.get("command").is_none());

        let invalid = serde_json::from_value::<OperationStartRequest>(serde_json::json!({
            "taskId":"system.overview",
            "taskVersion":2,
            "parameters":{},
            "confirmedPreviewId":null,
            "command":"id"
        }));
        assert!(invalid.is_err());

        let invalid_batch = serde_json::from_value::<OperationBatchRequest>(serde_json::json!({
            "serverIds":["server-1"],
            "taskId":"system.overview",
            "taskVersion":2,
            "parameters":{},
            "concurrency":20
        }));
        assert!(invalid_batch.is_err());
        assert!(serde_json::from_value::<ReportFormat>(serde_json::json!("json")).is_ok());
        assert!(serde_json::from_value::<ReportFormat>(serde_json::json!("../report")).is_err());
    }

    #[test]
    fn ipc_confirmation_request_is_token_bound_and_rejects_sensitive_fields() {
        let token = Uuid::new_v4();
        let request = OperationConfirmRequest {
            task_id: "service.restart".into(),
            task_version: 2,
            parameters: serde_json::json!({"service":"nginx"}),
            confirmation_token: token,
        };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["confirmationToken"], token.to_string());
        for forbidden in ["command", "localPath", "remoteScript", "sudoPassword"] {
            assert!(value.get(forbidden).is_none());
        }

        for forbidden in ["command", "localPath", "remoteScript", "sudoPassword"] {
            let mut value = value.clone();
            value[forbidden] = serde_json::json!("must-not-cross-ipc");
            assert!(serde_json::from_value::<OperationConfirmRequest>(value).is_err());
        }
    }
}
