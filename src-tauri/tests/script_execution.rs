use std::sync::Arc;

use qingzhou_ssh_lib::{
    core::{
        database::Database,
        secret_protector::SecretProtector,
        ssh::executor::VecEventSink,
        tasks::{ParameterDefinition, ParameterKind, RiskLevel},
    },
    domain::{
        script::{NewPersonalScript, NewScriptVersion},
        server::{CreateServerRequest, CredentialInput},
    },
    error::{AppError, AppResult},
    services::app_services::AppServices,
};
use serde_json::json;

struct XorProtector;

impl SecretProtector for XorProtector {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xA5).collect())
    }

    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xA5).collect())
    }
}

fn new_script(body: &str, enabled: bool) -> NewPersonalScript {
    NewPersonalScript {
        title: "个人巡检脚本".into(),
        category: "系统维护".into(),
        tags: vec!["巡检".into()],
        is_favorite: false,
        is_enabled: enabled,
        version: NewScriptVersion {
            body: body.into(),
            parameters: json!([]),
            scan_summary: json!({}),
            timeout_seconds: 30,
            shell: qingzhou_ssh_lib::domain::script::ScriptShell::PosixSh,
            compatibility: qingzhou_ssh_lib::domain::script::ScriptCompatibility::for_shell(
                qingzhou_ssh_lib::domain::script::ScriptShell::PosixSh,
            ),
        },
    }
}

async fn fixture() -> (tempfile::TempDir, AppServices, String) {
    let root = tempfile::tempdir().unwrap();
    let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "脚本测试服务器".into(),
            host: "127.0.0.1".into(),
            port: 1,
            username: "tester".into(),
            credential: CredentialInput::Password {
                password: "script-credential-canary".into(),
            },
        })
        .await
        .unwrap();
    (root, services, server.id)
}

#[tokio::test]
async fn personal_script_is_dangerous_unrecoverable_version_locked_and_not_logged() {
    let (root, services, server_id) = fixture().await;
    let scripts = services.script_service();
    let mut draft = new_script("echo script-body-canary", true);
    draft.version.parameters = serde_json::to_value(vec![ParameterDefinition {
        name: "PASSWORD".into(),
        label: "临时密码".into(),
        description: "仅在运行内存中使用".into(),
        kind: ParameterKind::String {
            min_length: 1,
            max_length: 128,
            multiline: false,
        },
        required: true,
        default_value: None,
        sensitive: true,
    }])
    .unwrap();
    let created = scripts.create(draft).await.unwrap();
    let v1 = created.active_version.id;
    let preview = scripts
        .preview_run(
            created.definition.id,
            &server_id,
            json!({"PASSWORD": "parameter-secret-canary"}),
        )
        .await
        .unwrap();
    assert_eq!(preview.risk_level, RiskLevel::Dangerous);
    assert!(!preview.automatic_rollback_available);
    assert!(preview.warning.contains("不可自动回滚"));
    assert!(!serde_json::to_string(&preview)
        .unwrap()
        .contains("script-body-canary"));

    assert!(matches!(
        scripts
            .confirm_run(
                preview.preview_id,
                uuid::Uuid::new_v4(),
                &mut VecEventSink::default()
            )
            .await,
        Err(AppError::ScriptConfirmationRequired(_))
    ));

    let v2 = scripts
        .save_version(
            created.definition.id,
            NewScriptVersion {
                body: "echo version-two-canary".into(),
                parameters: json!([]),
                scan_summary: json!({}),
                timeout_seconds: 30,
                shell: qingzhou_ssh_lib::domain::script::ScriptShell::PosixSh,
                compatibility: qingzhou_ssh_lib::domain::script::ScriptCompatibility::for_shell(
                    qingzhou_ssh_lib::domain::script::ScriptShell::PosixSh,
                ),
            },
        )
        .await
        .unwrap();
    assert_ne!(v1, v2.id);

    let mut events = VecEventSink::default();
    let result = scripts
        .confirm_run(preview.preview_id, preview.confirmation_token, &mut events)
        .await
        .unwrap();
    assert_eq!(result.script_version_id, v1);
    assert_eq!(result.execution.record.task_id, "script.personal");
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("script-body-canary"));
    assert!(!serde_json::to_string(&events.events)
        .unwrap()
        .contains("script-body-canary"));
    assert!(matches!(
        scripts
            .confirm_run(
                preview.preview_id,
                preview.confirmation_token,
                &mut VecEventSink::default()
            )
            .await,
        Err(AppError::ScriptConfirmationRequired(_))
    ));

    let database = Database::open(root.path()).await.unwrap();
    let version_id: String =
        sqlx::query_scalar("SELECT version_id FROM script_runs WHERE operation_run_id=?")
            .bind(result.operation_run_id.to_string())
            .fetch_one(database.pool())
            .await
            .unwrap();
    assert_eq!(version_id, v1.to_string());
    let history: Vec<String> = sqlx::query_scalar(
        "SELECT COALESCE(parameters_summary,'') FROM executions UNION ALL SELECT COALESCE(parameters_summary,'') FROM operation_runs",
    )
    .fetch_all(database.pool())
    .await
    .unwrap();
    assert!(history
        .iter()
        .all(|summary| !summary.contains("script-body-canary")
            && !summary.contains("parameter-secret-canary")));
}

#[tokio::test]
async fn disabled_deleted_and_invalid_parameter_scripts_cannot_be_previewed() {
    let (_root, services, server_id) = fixture().await;
    let scripts = services.script_service();
    let disabled = scripts
        .create(new_script("echo disabled", false))
        .await
        .unwrap();
    assert!(scripts
        .preview_run(disabled.definition.id, &server_id, json!({}))
        .await
        .is_err());

    scripts
        .set_enabled(disabled.definition.id, true)
        .await
        .unwrap();
    scripts.delete(disabled.definition.id).await.unwrap();
    assert!(scripts
        .preview_run(disabled.definition.id, &server_id, json!({}))
        .await
        .is_err());

    let mut required = new_script("printf '%s' \"$QZ_PARAM_TARGET\"", true);
    required.version.parameters = serde_json::to_value(vec![ParameterDefinition {
        name: "TARGET".into(),
        label: "目标名称".into(),
        description: "运行前填写".into(),
        kind: ParameterKind::String {
            min_length: 1,
            max_length: 20,
            multiline: false,
        },
        required: true,
        default_value: None,
        sensitive: false,
    }])
    .unwrap();
    let required = scripts.create(required).await.unwrap();
    assert!(scripts
        .preview_run(required.definition.id, &server_id, json!({}))
        .await
        .is_err());

    let cancellable = scripts
        .create(new_script("echo cancellable", true))
        .await
        .unwrap();
    let preview = scripts
        .preview_run(cancellable.definition.id, &server_id, json!({}))
        .await
        .unwrap();
    scripts.cancel_run(preview.preview_id).await.unwrap();
    assert!(matches!(
        scripts
            .confirm_run(
                preview.preview_id,
                preview.confirmation_token,
                &mut VecEventSink::default()
            )
            .await,
        Err(AppError::ScriptConfirmationRequired(_))
    ));

    let preview = scripts
        .preview_run(cancellable.definition.id, &server_id, json!({}))
        .await
        .unwrap();
    scripts
        .set_enabled(cancellable.definition.id, false)
        .await
        .unwrap();
    assert!(scripts
        .confirm_run(
            preview.preview_id,
            preview.confirmation_token,
            &mut VecEventSink::default()
        )
        .await
        .is_err());
}
