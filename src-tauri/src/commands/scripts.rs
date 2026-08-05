use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{ipc::Channel, State};
use uuid::Uuid;

use crate::{
    core::{scripts::package::ScriptPackageExport, tasks::ParameterDefinition},
    domain::{
        events::ExecutionEvent,
        script::{
            NewPersonalScript, NewScriptVersion, ScriptDetails, ScriptListFilter,
            ScriptMetadataUpdate, ScriptSummary, ScriptVersion,
        },
    },
    error::AppResult,
    services::script_service::{ScriptRunPreview, ScriptRunResult},
    state::AppState,
};

use super::{services, ChannelEventSink};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatePersonalScriptRequest {
    pub title: String,
    pub category: String,
    pub tags: Vec<String>,
    pub body: String,
    pub parameters: Vec<ParameterDefinition>,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavePersonalScriptVersionRequest {
    pub body: String,
    pub parameters: Vec<ParameterDefinition>,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdatePersonalScriptMetadataRequest {
    pub title: String,
    pub category: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfirmPersonalScriptRunRequest {
    pub preview_id: Uuid,
    pub confirmation_token: Uuid,
}

#[tauri::command]
pub async fn list_personal_scripts(
    filter: ScriptListFilter,
    state: State<'_, AppState>,
) -> AppResult<Vec<ScriptSummary>> {
    services(&state).await?.script_service().list(filter).await
}

#[tauri::command]
pub async fn get_personal_script_for_editor(
    script_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<Option<ScriptDetails>> {
    services(&state)
        .await?
        .script_service()
        .get_for_editor(script_id)
        .await
}

#[tauri::command]
pub async fn list_personal_script_versions(
    script_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<Vec<ScriptVersion>> {
    services(&state)
        .await?
        .script_service()
        .list_versions(script_id)
        .await
}

#[tauri::command]
pub async fn create_personal_script(
    request: CreatePersonalScriptRequest,
    state: State<'_, AppState>,
) -> AppResult<ScriptDetails> {
    let parameters = serde_json::to_value(request.parameters)
        .map_err(|error| crate::error::AppError::Serialization(error.to_string()))?;
    services(&state)
        .await?
        .script_service()
        .create(NewPersonalScript {
            title: request.title,
            category: request.category,
            tags: request.tags,
            is_favorite: false,
            is_enabled: false,
            version: NewScriptVersion {
                body: request.body,
                parameters,
                scan_summary: Value::Null,
                timeout_seconds: request.timeout_seconds,
            },
        })
        .await
}

#[tauri::command]
pub async fn save_personal_script_version(
    script_id: Uuid,
    request: SavePersonalScriptVersionRequest,
    state: State<'_, AppState>,
) -> AppResult<ScriptVersion> {
    let parameters = serde_json::to_value(request.parameters)
        .map_err(|error| crate::error::AppError::Serialization(error.to_string()))?;
    services(&state)
        .await?
        .script_service()
        .save_version(
            script_id,
            NewScriptVersion {
                body: request.body,
                parameters,
                scan_summary: Value::Null,
                timeout_seconds: request.timeout_seconds,
            },
        )
        .await
}

#[tauri::command]
pub async fn update_personal_script_metadata(
    script_id: Uuid,
    request: UpdatePersonalScriptMetadataRequest,
    state: State<'_, AppState>,
) -> AppResult<()> {
    services(&state)
        .await?
        .script_service()
        .update_metadata(
            script_id,
            ScriptMetadataUpdate {
                title: request.title,
                category: request.category,
                tags: request.tags,
            },
        )
        .await
}

#[tauri::command]
pub async fn copy_personal_script(
    script_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<ScriptDetails> {
    services(&state)
        .await?
        .script_service()
        .copy(script_id)
        .await
}

#[tauri::command]
pub async fn delete_personal_script(script_id: Uuid, state: State<'_, AppState>) -> AppResult<()> {
    services(&state)
        .await?
        .script_service()
        .delete(script_id)
        .await
}

#[tauri::command]
pub async fn set_personal_script_favorite(
    script_id: Uuid,
    favorite: bool,
    state: State<'_, AppState>,
) -> AppResult<()> {
    services(&state)
        .await?
        .script_service()
        .set_favorite(script_id, favorite)
        .await
}

#[tauri::command]
pub async fn set_personal_script_enabled(
    script_id: Uuid,
    enabled: bool,
    state: State<'_, AppState>,
) -> AppResult<()> {
    services(&state)
        .await?
        .script_service()
        .set_enabled(script_id, enabled)
        .await
}

#[tauri::command]
pub async fn import_personal_script(
    package_json: String,
    state: State<'_, AppState>,
) -> AppResult<ScriptDetails> {
    services(&state)
        .await?
        .script_service()
        .import(package_json.as_bytes())
        .await
}

#[tauri::command]
pub async fn export_personal_script(
    script_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<ScriptPackageExport> {
    services(&state)
        .await?
        .script_service()
        .export(script_id)
        .await
}

#[tauri::command]
pub async fn preview_personal_script_run(
    script_id: Uuid,
    server_id: String,
    parameter_values: Value,
    state: State<'_, AppState>,
) -> AppResult<ScriptRunPreview> {
    services(&state)
        .await?
        .script_service()
        .preview_run(script_id, &server_id, parameter_values)
        .await
}

#[tauri::command]
pub async fn confirm_personal_script_run(
    request: ConfirmPersonalScriptRunRequest,
    on_event: Channel<ExecutionEvent>,
    state: State<'_, AppState>,
) -> AppResult<ScriptRunResult> {
    let mut events = ChannelEventSink(on_event);
    services(&state)
        .await?
        .script_service()
        .confirm_run(request.preview_id, request.confirmation_token, &mut events)
        .await
}

#[tauri::command]
pub async fn cancel_personal_script_run(
    operation_run_id: Uuid,
    state: State<'_, AppState>,
) -> AppResult<()> {
    services(&state)
        .await?
        .script_service()
        .cancel_run(operation_run_id)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_confirm_requests_reject_execution_escape_hatches() {
        let create = serde_json::json!({
            "title":"巡检",
            "category":"系统",
            "tags":[],
            "body":"echo ok",
            "parameters":[],
            "timeoutSeconds":30
        });
        assert!(serde_json::from_value::<CreatePersonalScriptRequest>(create.clone()).is_ok());
        for forbidden in [
            "riskLevel",
            "rollbackAvailable",
            "command",
            "localPath",
            "serverIds",
        ] {
            let mut value = create.clone();
            value[forbidden] = serde_json::json!("forbidden");
            assert!(serde_json::from_value::<CreatePersonalScriptRequest>(value).is_err());
        }

        let confirm = serde_json::json!({
            "previewId":Uuid::new_v4(),
            "confirmationToken":Uuid::new_v4()
        });
        assert!(serde_json::from_value::<ConfirmPersonalScriptRunRequest>(confirm.clone()).is_ok());
        for forbidden in ["scriptId", "serverId", "command", "localPath", "riskLevel"] {
            let mut value = confirm.clone();
            value[forbidden] = serde_json::json!("forbidden");
            assert!(serde_json::from_value::<ConfirmPersonalScriptRunRequest>(value).is_err());
        }
    }
}
