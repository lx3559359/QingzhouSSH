use qingzhou_ssh_lib::{
    core::scripts::package::{export_script_package, import_script_package},
    domain::script::{ScriptDefinition, ScriptDetails, ScriptVersion},
    error::AppError,
};
use serde_json::json;
use uuid::Uuid;

fn valid_package() -> String {
    json!({
        "schemaVersion": 1,
        "exportedAt": 1_786_000_000_000_i64,
        "script": {
            "title": "服务巡检",
            "category": "系统维护",
            "tags": ["巡检"],
            "body": "printf '%s\\n' ok",
            "parameters": []
        }
    })
    .to_string()
}

fn details(body: &str) -> ScriptDetails {
    let definition_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    ScriptDetails {
        definition: ScriptDefinition {
            id: definition_id,
            title: "服务巡检".into(),
            category: "系统维护".into(),
            tags: vec!["巡检".into()],
            is_favorite: true,
            is_enabled: true,
            active_version_id: version_id,
            created_at: 1,
            updated_at: 2,
            deleted_at: None,
        },
        active_version: ScriptVersion {
            id: version_id,
            definition_id,
            version_number: 7,
            body: body.into(),
            body_sha256: "0".repeat(64),
            parameters: json!([]),
            scan_summary: json!({"warningCount": 0}),
            timeout_seconds: 120,
            created_at: 2,
        },
    }
}

#[test]
fn import_rejects_unknown_version_forbidden_state_and_oversized_input() {
    let unsupported = valid_package().replace("\"schemaVersion\":1", "\"schemaVersion\":2");
    assert!(matches!(
        import_script_package(unsupported.as_bytes()),
        Err(AppError::UnsupportedScriptPackage(_))
    ));

    for forbidden in [
        "serverId",
        "credentials",
        "history",
        "runs",
        "localPath",
        "dataRoot",
        "privateKey",
        "password",
        "token",
    ] {
        let mut value: serde_json::Value = serde_json::from_str(&valid_package()).unwrap();
        value["script"][forbidden] = json!("forbidden");
        assert!(matches!(
            import_script_package(value.to_string().as_bytes()),
            Err(AppError::ForbiddenScriptField(_))
        ));
    }

    assert!(import_script_package(&vec![b'x'; 2 * 1024 * 1024 + 1]).is_err());
}

#[test]
fn import_rejects_embedded_private_keys_and_credentials() {
    for body in [
        "-----BEGIN OPENSSH PRIVATE KEY-----\nsecret",
        "password=hunter2\necho unsafe",
        "API_TOKEN = abc123",
    ] {
        let mut value: serde_json::Value = serde_json::from_str(&valid_package()).unwrap();
        value["script"]["body"] = json!(body);
        assert!(matches!(
            import_script_package(value.to_string().as_bytes()),
            Err(AppError::UnsafeScriptPackage(_))
        ));
    }
}

#[test]
fn imported_script_is_a_fresh_disabled_definition() {
    let imported = import_script_package(valid_package().as_bytes()).unwrap();
    assert_eq!(imported.title, "服务巡检");
    assert_eq!(imported.version.body, "printf '%s\\n' ok");
    assert_eq!(imported.version.timeout_seconds, 300);
    assert!(!imported.is_enabled);
    assert!(!imported.is_favorite);
}

#[tokio::test]
async fn export_is_atomic_confined_and_excludes_external_state() {
    let root = tempfile::tempdir().unwrap();
    let script = details("echo package-body-canary");
    let exported = export_script_package(root.path(), &script).await.unwrap();

    assert!(exported
        .relative_path
        .starts_with("downloads/scripts/script-"));
    assert!(exported.relative_path.ends_with(".json"));
    assert_eq!(exported.sha256.len(), 64);
    let absolute = root.path().join(&exported.relative_path);
    assert!(absolute.starts_with(root.path()));
    let json = tokio::fs::read_to_string(absolute).await.unwrap();
    assert!(json.contains("package-body-canary"));
    let forbidden = vec![
        "serverId".to_string(),
        "credentials".to_string(),
        "history".to_string(),
        "runs".to_string(),
        "localPath".to_string(),
        "dataRoot".to_string(),
        "privateKey".to_string(),
        "password".to_string(),
        "token".to_string(),
        script.definition.id.to_string(),
        script.active_version.id.to_string(),
    ];
    for forbidden in forbidden {
        assert!(!json.contains(&forbidden));
    }
    let leftovers = std::fs::read_dir(root.path().join("downloads/scripts"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".partial"))
        .count();
    assert_eq!(leftovers, 0);
}
