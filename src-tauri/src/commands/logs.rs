use tauri::{ipc::Channel, State};
use uuid::Uuid;

use crate::{
    core::logs::{LogResultPage, LogSearchRequest},
    domain::{events::ExecutionEvent, execution::ExecutionDetails},
    error::AppResult,
    state::AppState,
};

use super::{services, ChannelEventSink};

#[tauri::command]
pub async fn search_logs(
    server_id: String,
    request: LogSearchRequest,
    on_event: Channel<ExecutionEvent>,
    state: State<'_, AppState>,
) -> AppResult<ExecutionDetails> {
    let mut events = ChannelEventSink(on_event);
    services(&state)
        .await?
        .search_logs(&server_id, request, &mut events)
        .await
}

#[tauri::command]
pub async fn read_log_result_page(
    execution_id: Uuid,
    cursor: Option<String>,
    page_size: usize,
    state: State<'_, AppState>,
) -> AppResult<LogResultPage> {
    services(&state)
        .await?
        .read_log_result_page(execution_id, cursor.as_deref(), page_size)
        .await
}

#[tauri::command]
pub async fn download_log_result(
    execution_id: Uuid,
    suggested_name: String,
    state: State<'_, AppState>,
) -> AppResult<String> {
    services(&state)
        .await?
        .download_log_result(execution_id, &suggested_name)
        .await
}
