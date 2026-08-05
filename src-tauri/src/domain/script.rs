use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptDefinition {
    pub id: Uuid,
    pub title: String,
    pub category: String,
    pub tags: Vec<String>,
    pub is_favorite: bool,
    pub is_enabled: bool,
    pub active_version_id: Uuid,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptVersion {
    pub id: Uuid,
    pub definition_id: Uuid,
    pub version_number: u32,
    pub body: String,
    pub body_sha256: String,
    pub parameters: Value,
    pub scan_summary: Value,
    pub timeout_seconds: u64,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptDetails {
    pub definition: ScriptDefinition,
    pub active_version: ScriptVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptSummary {
    pub id: Uuid,
    pub title: String,
    pub category: String,
    pub tags: Vec<String>,
    pub is_favorite: bool,
    pub is_enabled: bool,
    pub active_version_id: Uuid,
    pub active_version_number: u32,
    pub body_sha256: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewScriptVersion {
    pub body: String,
    pub parameters: Value,
    pub scan_summary: Value,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct NewPersonalScript {
    pub title: String,
    pub category: String,
    pub tags: Vec<String>,
    pub is_favorite: bool,
    pub is_enabled: bool,
    pub version: NewScriptVersion,
}

#[derive(Debug, Clone)]
pub struct ScriptMetadataUpdate {
    pub title: String,
    pub category: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScriptListFilter {
    pub query: Option<String>,
    pub category: Option<String>,
    pub tag: Option<String>,
    pub favorite: Option<bool>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptRunReference {
    pub id: Uuid,
    pub definition_id: Uuid,
    pub version_id: Uuid,
    pub operation_run_id: Uuid,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewScriptRunReference {
    pub definition_id: Uuid,
    pub version_id: Uuid,
    pub operation_run_id: Uuid,
}
