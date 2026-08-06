use std::time::Duration;
use std::{path::PathBuf, sync::atomic::Ordering};

use serde::Deserialize;
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::{
    core::data_migration::{DataMigrationJournal, DataMigrationPreview},
    domain::update::UpdatePhase,
    error::{AppError, AppResult},
    state::AppState,
};

use super::{services, updater};

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataRootFolderKind {
    Current,
    LastSource,
}

#[tauri::command]
pub async fn preflight_data_root_migration(
    target_path: String,
    state: State<'_, AppState>,
) -> AppResult<DataMigrationPreview> {
    let services = services(&state).await?;
    services.ensure_idle_for_data_migration().await?;
    services
        .data_migration_service()
        .preflight(
            &PathBuf::from(target_path),
            services.data_root_source(),
            services.data_root_mutable(),
        )
        .await
}

#[tauri::command]
pub async fn preflight_retry_data_root_migration(
    state: State<'_, AppState>,
) -> AppResult<DataMigrationPreview> {
    let services = services(&state).await?;
    services.ensure_idle_for_data_migration().await?;
    services
        .data_migration_service()
        .preflight_retry(services.data_root_source(), services.data_root_mutable())
        .await
}

#[tauri::command]
pub async fn preflight_portable_default_data_root_migration(
    state: State<'_, AppState>,
) -> AppResult<DataMigrationPreview> {
    let services = services(&state).await?;
    services.ensure_idle_for_data_migration().await?;
    if services.data_root_source() != crate::core::data_root::DataRootSource::PortableCustom {
        return Err(AppError::Validation(
            "只有已改为自定义目录的便携版可以恢复程序旁数据目录".into(),
        ));
    }
    let executable = std::env::current_exe()?;
    let target = executable
        .parent()
        .ok_or_else(|| AppError::Validation("无法确定程序目录".into()))?
        .join("data");
    services
        .data_migration_service()
        .preflight(
            &target,
            services.data_root_source(),
            services.data_root_mutable(),
        )
        .await
}

#[tauri::command]
pub async fn start_data_root_migration(
    preview_id: Uuid,
    confirmation_token: Uuid,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<DataMigrationJournal> {
    let services = services(&state).await?;
    let updater = updater(&state).await?;
    services.ensure_idle_for_data_migration().await?;
    let update_phase = updater
        .status()
        .await
        .map_err(|_| AppError::Update("无法确认更新服务是否空闲".into()))?
        .phase;
    if matches!(
        update_phase,
        UpdatePhase::Checking | UpdatePhase::Downloading | UpdatePhase::Installing
    ) {
        return Err(AppError::Validation(
            "更新检查、下载或安装仍在进行，请完成后再迁移数据目录".into(),
        ));
    }
    if state
        .migration_starting
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(AppError::Validation("数据目录迁移已经开始".into()));
    }
    let result = services
        .data_migration_service()
        .start(
            preview_id,
            confirmation_token,
            services.data_root_source(),
            services.data_root_mutable(),
        )
        .await;
    match result {
        Ok(journal) => {
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(350)).await;
                app.exit(0);
            });
            Ok(journal)
        }
        Err(error) => {
            state.migration_starting.store(false, Ordering::SeqCst);
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn get_data_root_migration_status(
    state: State<'_, AppState>,
) -> AppResult<Option<DataMigrationJournal>> {
    services(&state).await?.data_migration_service().status()
}

#[tauri::command]
pub async fn acknowledge_data_root_migration(
    migration_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<DataMigrationJournal> {
    services(&state)
        .await?
        .data_migration_service()
        .acknowledge(migration_id)
}

#[tauri::command]
pub async fn open_data_root_folder(
    kind: DataRootFolderKind,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let migration = services(&state).await?.data_migration_service();
    match kind {
        DataRootFolderKind::Current => migration.open_current_folder(),
        DataRootFolderKind::LastSource => migration.open_last_source_folder(),
    }
}
