use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    core::{
        scripts::validation::{
            scan_script_body_for, validate_script_metadata, validate_script_parameters,
        },
        tasks::ParameterDefinition,
    },
    domain::{
        execution::now_millis,
        script::{
            NewPersonalScript, NewScriptVersion, ScriptCompatibility, ScriptDetails, ScriptShell,
        },
    },
    error::{AppError, AppResult},
};

const SCRIPT_PACKAGE_SCHEMA_VERSION: u32 = 2;
const MAX_SCRIPT_PACKAGE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptPackageExport {
    pub relative_path: String,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScriptPackage {
    schema_version: u32,
    exported_at: i64,
    script: ScriptPackageDefinition,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScriptPackageDefinition {
    title: String,
    category: String,
    tags: Vec<String>,
    body: String,
    parameters: Vec<ParameterDefinition>,
    #[serde(default)]
    shell: Option<ScriptShell>,
    #[serde(default)]
    compatibility: Option<ScriptCompatibility>,
}

pub fn import_script_package(bytes: &[u8]) -> AppResult<NewPersonalScript> {
    if bytes.len() > MAX_SCRIPT_PACKAGE_BYTES {
        return Err(AppError::Validation(
            "脚本包不能超过 2 MiB，请缩小后重试".into(),
        ));
    }
    let raw: Value = serde_json::from_slice(bytes)
        .map_err(|error| AppError::Serialization(format!("脚本包 JSON 无效：{error}")))?;
    reject_forbidden_fields(&raw, "$")?;
    let schema_version = raw
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    if !matches!(schema_version, 1 | 2) {
        return Err(AppError::UnsupportedScriptPackage(format!(
            "仅支持 schemaVersion 1 或 2，当前为 {schema_version}"
        )));
    }
    let package: ScriptPackage = serde_json::from_value(raw)
        .map_err(|error| AppError::Serialization(format!("脚本包结构无效：{error}")))?;
    validate_script_metadata(
        &package.script.title,
        &package.script.category,
        &package.script.tags,
    )?;
    validate_script_parameters(&package.script.parameters)?;
    reject_sensitive_body(&package.script.body)?;
    let shell = package.script.shell.unwrap_or_default();
    let scan = scan_script_body_for(shell, &package.script.body)?;
    let parameters = serde_json::to_value(package.script.parameters)
        .map_err(|error| AppError::Serialization(error.to_string()))?;
    let scan_summary =
        serde_json::to_value(scan).map_err(|error| AppError::Serialization(error.to_string()))?;
    let compatibility = package
        .script
        .compatibility
        .unwrap_or_else(|| ScriptCompatibility::for_shell(shell));
    if compatibility != ScriptCompatibility::for_shell(shell) {
        return Err(AppError::Validation(
            "脚本包兼容性声明必须与 Shell 一致".into(),
        ));
    }
    Ok(NewPersonalScript {
        title: package.script.title,
        category: package.script.category,
        tags: package.script.tags,
        is_favorite: false,
        is_enabled: false,
        version: NewScriptVersion {
            body: package.script.body,
            parameters,
            scan_summary,
            timeout_seconds: 300,
            shell,
            compatibility,
        },
    })
}

pub async fn export_script_package(
    data_root: &Path,
    details: &ScriptDetails,
) -> AppResult<ScriptPackageExport> {
    if !data_root.is_absolute() {
        return Err(AppError::Validation("项目数据目录必须是绝对路径".into()));
    }
    validate_script_metadata(
        &details.definition.title,
        &details.definition.category,
        &details.definition.tags,
    )?;
    let parameters: Vec<ParameterDefinition> =
        serde_json::from_value(details.active_version.parameters.clone())
            .map_err(|_| AppError::Validation("脚本参数定义格式无效".into()))?;
    validate_script_parameters(&parameters)?;
    scan_script_body_for(details.active_version.shell, &details.active_version.body)?;

    let package = ScriptPackage {
        schema_version: SCRIPT_PACKAGE_SCHEMA_VERSION,
        exported_at: now_millis(),
        script: ScriptPackageDefinition {
            title: details.definition.title.clone(),
            category: details.definition.category.clone(),
            tags: details.definition.tags.clone(),
            body: details.active_version.body.clone(),
            parameters,
            shell: Some(details.active_version.shell),
            compatibility: Some(details.active_version.compatibility.clone()),
        },
    };
    let bytes = serde_json::to_vec_pretty(&package)
        .map_err(|error| AppError::Serialization(error.to_string()))?;
    if bytes.len() > MAX_SCRIPT_PACKAGE_BYTES {
        return Err(AppError::Validation(
            "导出的脚本包超过 2 MiB，无法安全写入".into(),
        ));
    }

    let directory = data_root.join("downloads").join("scripts");
    tokio::fs::create_dir_all(&directory).await?;
    let file_name = format!("script-{}.json", Uuid::new_v4());
    let destination = directory.join(&file_name);
    let partial = directory.join(format!(".{file_name}.partial"));
    let write_result = async {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
            .await?;
        file.write_all(&bytes).await?;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&partial, &destination).await
    }
    .await;
    if let Err(error) = write_result {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(AppError::Io(error));
    }

    Ok(ScriptPackageExport {
        relative_path: format!("downloads/scripts/{file_name}"),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        size_bytes: u64::try_from(bytes.len())
            .map_err(|_| AppError::Integrity("脚本包大小超出支持范围".into()))?,
    })
}

fn reject_forbidden_fields(value: &Value, path: &str) -> AppResult<()> {
    match value {
        Value::Object(object) => {
            for (key, nested) in object {
                let normalized = key
                    .chars()
                    .filter(|character| character.is_ascii_alphanumeric())
                    .flat_map(char::to_lowercase)
                    .collect::<String>();
                if is_forbidden_field(&normalized) {
                    return Err(AppError::ForbiddenScriptField(format!(
                        "{path}.{key} 不允许出现在脚本包中"
                    )));
                }
                reject_forbidden_fields(nested, &format!("{path}.{key}"))?;
            }
        }
        Value::Array(values) => {
            for (index, nested) in values.iter().enumerate() {
                reject_forbidden_fields(nested, &format!("{path}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_forbidden_field(field: &str) -> bool {
    matches!(
        field,
        "server"
            | "servers"
            | "serverid"
            | "serverids"
            | "credential"
            | "credentials"
            | "credentialid"
            | "history"
            | "run"
            | "runs"
            | "operationruns"
            | "localpath"
            | "dataroot"
            | "privatekey"
            | "password"
            | "token"
            | "apikey"
            | "secret"
    )
}

fn reject_sensitive_body(body: &str) -> AppResult<()> {
    let upper = body.to_ascii_uppercase();
    if upper.contains("-----BEGIN ") && upper.contains("PRIVATE KEY-----") {
        return Err(AppError::UnsafeScriptPackage(
            "检测到私钥内容，请删除凭据后再导入".into(),
        ));
    }
    for (index, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        let name = name
            .trim()
            .trim_start_matches("export ")
            .to_ascii_uppercase();
        let value = value.trim().trim_matches(['\'', '"']);
        let sensitive_name = ["PASSWORD", "TOKEN", "API_KEY", "PRIVATE_KEY", "SECRET"]
            .iter()
            .any(|suffix| name == *suffix || name.ends_with(&format!("_{suffix}")));
        if sensitive_name && !value.is_empty() && !value.starts_with('$') {
            return Err(AppError::UnsafeScriptPackage(format!(
                "第 {} 行疑似包含明文凭据赋值，请删除后再导入",
                index + 1
            )));
        }
    }
    Ok(())
}
