use std::path::PathBuf;

use tauri::{ipc::Channel, State};

use crate::{
    core::sftp::{DirectoryListing, DownloadRequest, UploadRequest},
    domain::{events::ExecutionEvent, execution::ExecutionDetails},
    error::AppResult,
    state::AppState,
};

use super::{services, ChannelEventSink};

#[tauri::command]
pub async fn list_local_directory(
    path: Option<PathBuf>,
    state: State<'_, AppState>,
) -> AppResult<DirectoryListing> {
    services(&state)
        .await?
        .list_local_directory(path.as_deref())
        .await
}

#[tauri::command]
pub async fn list_remote_directory(
    server_id: String,
    path: String,
    state: State<'_, AppState>,
) -> AppResult<DirectoryListing> {
    services(&state)
        .await?
        .list_remote_directory(&server_id, &path)
        .await
}

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
