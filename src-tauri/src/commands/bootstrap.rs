use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::{
    core::{
        data_migration::DataMigrationJournal, data_root::DataRootSource,
        data_root_store::system_data_root_store,
    },
    error::{AppError, AppResult},
    services::app_services::AppServices,
    services::update_service::UpdateManager,
    state::AppState,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state")]
pub enum BootstrapStatus {
    #[serde(rename = "needs_selection")]
    NeedsSelection,
    #[serde(rename = "ready")]
    Ready {
        #[serde(rename = "dataRoot")]
        data_root: String,
        #[serde(rename = "dataRootSource")]
        data_root_source: DataRootSource,
        #[serde(rename = "dataRootMutable")]
        data_root_mutable: bool,
        #[serde(rename = "lastDataMigration")]
        last_data_migration: Option<DataMigrationJournal>,
    },
}

impl BootstrapStatus {
    fn ready(services: &AppServices) -> Self {
        Self::Ready {
            data_root: services.data_root().to_string_lossy().into_owned(),
            data_root_source: services.data_root_source(),
            data_root_mutable: services.data_root_mutable(),
            last_data_migration: services.data_migration_service().status().ok().flatten(),
        }
    }
}

#[tauri::command]
pub async fn bootstrap_status(state: State<'_, AppState>) -> AppResult<BootstrapStatus> {
    let services = state.services.read().await;
    Ok(match services.as_ref() {
        Some(services) => BootstrapStatus::ready(services),
        None => BootstrapStatus::NeedsSelection,
    })
}

#[tauri::command]
pub async fn initialize_data_root(
    path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<BootstrapStatus> {
    if path.trim().is_empty() || path.contains('\0') {
        return Err(AppError::Validation("数据目录路径无效".into()));
    }
    let root = PathBuf::from(path);
    let mut services = state.services.write().await;
    if let Some(current) = services.as_ref() {
        return Ok(BootstrapStatus::ready(current));
    }

    let initialized = AppServices::open(&root).await?;
    let updater = UpdateManager::new(app.package_info().version.to_string(), &root, app.clone())
        .map_err(|_| AppError::Update("更新服务初始化失败".into()))?;
    system_data_root_store()?.save(&root)?;
    let status = BootstrapStatus::ready(&initialized);
    *services = Some(initialized);
    *state.updater.write().await = Some(updater);
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_contract_matches_the_frontend_discriminated_union() {
        assert_eq!(
            serde_json::to_value(BootstrapStatus::NeedsSelection).unwrap(),
            serde_json::json!({ "state": "needs_selection" })
        );
        assert_eq!(
            serde_json::to_value(BootstrapStatus::Ready {
                data_root: r"D:\QingzhouData".into(),
                data_root_source: DataRootSource::Platform,
                data_root_mutable: true,
                last_data_migration: None,
            })
            .unwrap(),
            serde_json::json!({
                "state": "ready",
                "dataRoot": r"D:\QingzhouData",
                "dataRootSource": "platform",
                "dataRootMutable": true,
                "lastDataMigration": null
            })
        );
    }
}
