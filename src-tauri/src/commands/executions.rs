use tauri::{ipc::Channel, State};
use uuid::Uuid;

use crate::{
    domain::{
        events::ExecutionEvent,
        execution::{ExecutionDetails, ExecutionFilter, ExecutionRecord},
    },
    error::AppResult,
    services::execution_service::{
        CustomExecutionRequest, TaskAvailability, TaskExecutionRequest, TaskLibrarySnapshot,
    },
    state::AppState,
};

use super::{services, ChannelEventSink};

#[tauri::command]
pub async fn list_task_definitions(
    server_id: String,
    state: State<'_, AppState>,
) -> AppResult<Vec<TaskAvailability>> {
    services(&state)
        .await?
        .list_task_definitions(&server_id)
        .await
}

#[tauri::command]
pub async fn get_task_library_snapshot(
    server_id: String,
    force_refresh: Option<bool>,
    state: State<'_, AppState>,
) -> AppResult<TaskLibrarySnapshot> {
    services(&state)
        .await?
        .get_task_library_snapshot(&server_id, force_refresh.unwrap_or(false))
        .await
}

#[tauri::command]
pub async fn start_task_execution(
    server_id: String,
    request: TaskExecutionRequest,
    on_event: Channel<ExecutionEvent>,
    state: State<'_, AppState>,
) -> AppResult<ExecutionDetails> {
    let mut events = ChannelEventSink(on_event);
    services(&state)
        .await?
        .start_task_execution(&server_id, request, &mut events)
        .await
}

#[tauri::command]
pub async fn start_custom_execution(
    server_id: String,
    request: CustomExecutionRequest,
    on_event: Channel<ExecutionEvent>,
    state: State<'_, AppState>,
) -> AppResult<ExecutionDetails> {
    let mut events = ChannelEventSink(on_event);
    services(&state)
        .await?
        .start_custom_execution(&server_id, request, &mut events)
        .await
}

#[tauri::command]
pub async fn cancel_execution(execution_id: Uuid, state: State<'_, AppState>) -> AppResult<()> {
    services(&state).await?.cancel_execution(execution_id).await
}

#[tauri::command]
pub async fn list_executions(
    filter: ExecutionFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<ExecutionRecord>> {
    services(&state).await?.list_executions(filter).await
}

#[tauri::command]
pub async fn get_execution(
    execution_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<Option<ExecutionDetails>> {
    services(&state).await?.get_execution(execution_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_request_contract_is_camel_case() {
        let value = serde_json::to_value(TaskExecutionRequest {
            task_id: "system.overview".into(),
            parameters: serde_json::json!({}),
            dangerous_confirmed: false,
        })
        .unwrap();
        assert_eq!(value["taskId"], "system.overview");
        assert_eq!(value["dangerousConfirmed"], false);
    }
}
