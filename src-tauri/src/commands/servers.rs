use tauri::State;

use crate::{
    core::{ssh::transport::HostKeyObservation, system_probe::SystemCapabilities},
    domain::server::{CreateServerRequest, ServerProfile},
    error::{AppError, AppResult},
    services::app_services::{AppServices, HostKeyCheck},
    state::AppState,
};

async fn services(state: &State<'_, AppState>) -> AppResult<AppServices> {
    state
        .services
        .read()
        .await
        .clone()
        .ok_or(AppError::NotReady)
}

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
