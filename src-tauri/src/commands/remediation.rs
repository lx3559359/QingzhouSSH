use tauri::{ipc::Channel, State};

use crate::{
    domain::events::ExecutionEvent,
    error::AppResult,
    services::{
        execution_service::TaskAvailability,
        task_remediation_service::{ConfirmTaskRemediationRequest, TaskRemediationPreview},
    },
    state::AppState,
};

use super::{services, ChannelEventSink};

#[tauri::command]
pub async fn preview_task_remediation(
    server_id: String,
    task_id: String,
    state: State<'_, AppState>,
) -> AppResult<TaskRemediationPreview> {
    services(&state)
        .await?
        .task_remediation_service()
        .preview(&server_id, &task_id)
        .await
}

#[tauri::command]
pub async fn confirm_task_remediation(
    server_id: String,
    request: ConfirmTaskRemediationRequest,
    on_event: Channel<ExecutionEvent>,
    state: State<'_, AppState>,
) -> AppResult<TaskAvailability> {
    let mut events = ChannelEventSink(on_event);
    services(&state)
        .await?
        .task_remediation_service()
        .confirm(&server_id, request, &mut events)
        .await
}
