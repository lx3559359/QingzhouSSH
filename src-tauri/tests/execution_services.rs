use qingzhou_ssh_lib::{
    core::{secret_protector::SecretProtector, ssh::executor::VecEventSink},
    domain::{
        execution::ExecutionStatus,
        server::{CreateServerRequest, CredentialInput},
    },
    error::{AppError, AppResult},
    services::{
        app_services::AppServices,
        execution_service::{CustomExecutionMode, CustomExecutionRequest, ExecutionRegistry},
    },
};
use std::sync::Arc;
use uuid::Uuid;

struct XorProtector;

impl SecretProtector for XorProtector {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xA5).collect())
    }

    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xA5).collect())
    }
}

#[tokio::test]
async fn registry_cancels_only_known_running_execution_and_cleans_up() {
    let registry = ExecutionRegistry::default();
    let execution_id = Uuid::new_v4();
    let token = registry.register(execution_id).await.unwrap();
    assert!(registry.contains(execution_id).await);
    assert!(!token.is_cancelled());

    registry.cancel(execution_id).await.unwrap();
    assert!(token.is_cancelled());
    registry.remove(execution_id).await;
    assert!(!registry.contains(execution_id).await);
    assert!(matches!(
        registry.cancel(execution_id).await,
        Err(AppError::Validation(_))
    ));
}

#[test]
fn custom_commands_and_scripts_are_noninteractive_and_require_confirmation() {
    let command = CustomExecutionRequest {
        mode: CustomExecutionMode::Command,
        content: "uptime".into(),
        timeout_seconds: 30,
        dangerous_confirmed: true,
        shell: qingzhou_ssh_lib::domain::script::ScriptShell::PosixSh,
    };
    assert_eq!(command.render().unwrap(), "uptime");

    let script = CustomExecutionRequest {
        mode: CustomExecutionMode::Script,
        content: "set -eu\nprintf 'ok\\n'".into(),
        timeout_seconds: 30,
        dangerous_confirmed: true,
        shell: qingzhou_ssh_lib::domain::script::ScriptShell::PosixSh,
    };
    let rendered = script.render().unwrap();
    assert!(rendered.starts_with("env sh -s <<'QZ_SCRIPT_"));
    assert!(rendered.contains("set -eu"));
    assert!(!rendered.contains("base64 -d"));

    for invalid in [
        CustomExecutionRequest {
            dangerous_confirmed: false,
            ..command.clone()
        },
        CustomExecutionRequest {
            content: "bad\0command".into(),
            ..command.clone()
        },
        CustomExecutionRequest {
            timeout_seconds: 0,
            ..command
        },
    ] {
        assert!(invalid.render().is_err());
    }
}

#[tokio::test]
async fn connection_failure_is_persisted_without_leaking_credential_canary() {
    let root = tempfile::tempdir().unwrap();
    let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "未信任服务器".into(),
            host: "127.0.0.1".into(),
            port: 22,
            username: "tester".into(),
            credential: CredentialInput::Password {
                password: "service-canary-password".into(),
            },
        })
        .await
        .unwrap();
    let mut events = VecEventSink::default();
    let details = services
        .start_custom_execution(
            &server.id,
            CustomExecutionRequest {
                mode: CustomExecutionMode::Command,
                content: "uptime".into(),
                timeout_seconds: 30,
                dangerous_confirmed: true,
                shell: qingzhou_ssh_lib::domain::script::ScriptShell::PosixSh,
            },
            &mut events,
        )
        .await
        .unwrap();
    assert_eq!(details.record.status, ExecutionStatus::Failed);
    assert_eq!(details.record.error_category.as_deref(), Some("security"));
    assert!(!events.events.is_empty());

    let database = std::fs::read(root.path().join("app.db")).unwrap();
    assert!(!String::from_utf8_lossy(&database).contains("service-canary-password"));
    for entry in walk_files(root.path().join("logs")) {
        let contents = std::fs::read(entry).unwrap();
        assert!(!String::from_utf8_lossy(&contents).contains("service-canary-password"));
    }
}

fn walk_files(root: std::path::PathBuf) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root];
    while let Some(path) = pending.pop() {
        if !path.exists() {
            continue;
        }
        if path.is_file() {
            files.push(path);
            continue;
        }
        for entry in std::fs::read_dir(path).unwrap() {
            pending.push(entry.unwrap().path());
        }
    }
    files
}
