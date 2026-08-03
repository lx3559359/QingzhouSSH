use tauri::{ipc::Channel, State};

use crate::{
    core::sftp::{DownloadRequest, UploadRequest},
    domain::{events::ExecutionEvent, execution::ExecutionDetails},
    error::AppResult,
    state::AppState,
};

use super::{services, ChannelEventSink};

#[tauri::command]
pub async fn upload_file(
    server_id: String,
    request: UploadRequest,
    on_event: Channel<ExecutionEvent>,
    state: State<'_, AppState>,
) -> AppResult<ExecutionDetails> {
    let mut events = ChannelEventSink(on_event);
    services(&state)
        .await?
        .upload_file(&server_id, request, &mut events)
        .await
}

#[tauri::command]
pub async fn download_file(
    server_id: String,
    request: DownloadRequest,
    on_event: Channel<ExecutionEvent>,
    state: State<'_, AppState>,
) -> AppResult<ExecutionDetails> {
    let mut events = ChannelEventSink(on_event);
    services(&state)
        .await?
        .download_file(&server_id, request, &mut events)
        .await
}
