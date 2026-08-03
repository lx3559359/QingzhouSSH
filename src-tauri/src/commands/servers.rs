use tauri::State;

use super::services;
use crate::{
    core::{ssh::transport::HostKeyObservation, system_probe::SystemCapabilities},
    domain::server::{CreateServerRequest, ServerProfile},
    error::AppResult,
    services::app_services::HostKeyCheck,
    state::AppState,
};

#[tauri::command]
pub async fn list_servers(state: State<'_, AppState>) -> AppResult<Vec<ServerProfile>> {
    services(&state).await?.list_servers().await
}

#[tauri::command]
pub async fn create_server(
    request: CreateServerRequest,
    state: State<'_, AppState>,
) -> AppResult<ServerProfile> {
    services(&state).await?.create_server(request).await
}

#[tauri::command]
pub async fn inspect_server_host_key(
    server_id: String,
    state: State<'_, AppState>,
) -> AppResult<HostKeyCheck> {
    services(&state).await?.inspect_host_key(&server_id).await
}

#[tauri::command]
pub async fn trust_server_host_key(
    server_id: String,
    observation: HostKeyObservation,
    state: State<'_, AppState>,
) -> AppResult<()> {
    services(&state)
        .await?
        .trust_host_key(&server_id, observation)
        .await
}

#[tauri::command]
pub async fn test_server_connection(
    server_id: String,
    state: State<'_, AppState>,
) -> AppResult<SystemCapabilities> {
    services(&state).await?.test_connection(&server_id).await
}
