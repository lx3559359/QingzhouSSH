use std::path::PathBuf;

use serde::Serialize;
use tauri::State;

use crate::{
    core::root_registry,
    error::{AppError, AppResult},
    services::app_services::AppServices,
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
    },
}

impl BootstrapStatus {
    fn ready(services: &AppServices) -> Self {
        Self::Ready {
            data_root: services.data_root().to_string_lossy().into_owned(),
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
    root_registry::save_data_root(&root)?;
    let status = BootstrapStatus::ready(&initialized);
    *services = Some(initialized);
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
            })
            .unwrap(),
            serde_json::json!({ "state": "ready", "dataRoot": r"D:\QingzhouData" })
        );
    }
}
