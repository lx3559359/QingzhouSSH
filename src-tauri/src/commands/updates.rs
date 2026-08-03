use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{ipc::Channel, State};

use crate::{
    commands::updater,
    core::updates::SourceFailureKind,
    error::{AppError, AppResult},
    services::update_service::{
        UpdateAdapterError, UpdateManagerError, UpdateProgressEvent, UpdateServiceError,
        UpdateStatus,
    },
    state::AppState,
};

#[tauri::command]
pub async fn get_update_status(state: State<'_, AppState>) -> AppResult<UpdateStatus> {
    updater(&state)
        .await?
        .status()
        .await
        .map_err(map_update_error)
}

#[tauri::command]
pub async fn set_auto_update_check(
    enabled: bool,
    state: State<'_, AppState>,
) -> AppResult<UpdateStatus> {
    updater(&state)
        .await?
        .set_auto_check(enabled)
        .await
        .map_err(map_update_error)
}

#[tauri::command]
pub async fn check_for_update(manual: bool, state: State<'_, AppState>) -> AppResult<UpdateStatus> {
    updater(&state)
        .await?
        .check(manual, unix_timestamp())
        .await
        .map_err(map_update_error)
}

#[tauri::command]
pub async fn download_update(
    on_event: Channel<UpdateProgressEvent>,
    state: State<'_, AppState>,
) -> AppResult<UpdateStatus> {
    updater(&state)
        .await?
        .download(Box::new(move |event| {
            let _ = on_event.send(event);
        }))
        .await
        .map_err(map_update_error)
}

#[tauri::command]
pub async fn install_update(
    confirmed: bool,
    state: State<'_, AppState>,
) -> AppResult<UpdateStatus> {
    updater(&state)
        .await?
        .install(confirmed)
        .await
        .map_err(map_update_error)
}

#[tauri::command]
pub async fn clear_downloaded_update(state: State<'_, AppState>) -> AppResult<UpdateStatus> {
    updater(&state)
        .await?
        .clear_downloaded()
        .await
        .map_err(map_update_error)
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn map_update_error(error: UpdateManagerError) -> AppError {
    let message = match error {
        UpdateManagerError::Source(error) => match error.kind {
            SourceFailureKind::Network => "更新网络暂时不可用",
            SourceFailureKind::NotFound => "更新源尚未发布版本清单",
            SourceFailureKind::Server => "更新服务暂时不可用",
            SourceFailureKind::InvalidManifest => "更新清单格式或内容无效",
            SourceFailureKind::Security => "更新源安全校验失败",
        },
        UpdateManagerError::Transition(_) => "当前更新状态不允许执行此操作",
        UpdateManagerError::Store(_) => "更新状态存储失败",
        UpdateManagerError::Service(error) => match error {
            UpdateServiceError::Adapter(UpdateAdapterError::Network) => "更新网络暂时不可用",
            UpdateServiceError::Adapter(UpdateAdapterError::Signature) => "更新签名验证失败",
            UpdateServiceError::Adapter(UpdateAdapterError::Manifest) => "更新清单校验失败",
            UpdateServiceError::Adapter(UpdateAdapterError::Install) => "更新安装器启动失败",
            UpdateServiceError::Integrity => "更新包完整性校验失败",
            UpdateServiceError::NotDownloaded => "更新包尚未完成下载",
            UpdateServiceError::ConfirmationRequired => "安装更新前必须明确确认",
            UpdateServiceError::Store(_) => "更新状态存储失败",
            UpdateServiceError::Io(_) => "更新文件操作失败",
        },
    };
    AppError::Update(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::updates::{SourceCheckError, SourceFailureKind};

    #[test]
    fn update_errors_are_safe_for_ipc() {
        let error = map_update_error(UpdateManagerError::Source(SourceCheckError::new(
            SourceFailureKind::Security,
            "https://example.invalid secret-signature",
        )));
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(error.code(), "update");
        assert!(!json.contains("example.invalid"));
        assert!(!json.contains("secret-signature"));
    }
}
