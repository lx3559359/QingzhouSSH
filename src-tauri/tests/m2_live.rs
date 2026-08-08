use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use qingzhou_ssh_lib::{
    core::{
        logs::{LogSearchRequest, LogSearchTarget, SearchResultItem},
        secret_protector::SecretProtector,
        sftp::{DownloadRequest, UploadRequest},
        ssh::executor::VecEventSink,
    },
    domain::{
        execution::{ExecutionFilter, ExecutionStatus},
        server::{CreateServerRequest, CredentialInput},
    },
    error::AppResult,
    services::{
        app_services::AppServices,
        execution_service::{CustomExecutionMode, CustomExecutionRequest, TaskExecutionRequest},
    },
};
use serde_json::json;

const PASSWORD_CANARY: &str = "testpass";
const PASSPHRASE_CANARY: &str = "fixture-passphrase";
const SCRIPT_CANARY: &str = "super-secret-script-token";

struct XorProtector;

impl SecretProtector for XorProtector {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xA5).collect())
    }

    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xA5).collect())
    }
}

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        if self.0.starts_with(project_root().join(".local/test-data")) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap()
}

async fn trusted_server(services: &AppServices, credential: CredentialInput, name: &str) -> String {
    let server = services
        .create_server(CreateServerRequest {
            name: name.into(),
            host: "127.0.0.1".into(),
            port: 2222,
            username: "testuser".into(),
            credential,
        })
        .await
        .unwrap();
    let check = services.inspect_host_key(&server.id).await.unwrap();
    services
        .trust_host_key(&server.id, check.observed)
        .await
        .unwrap();
    server.id
}

fn fixture_connection_count() -> u64 {
    std::fs::read_to_string(
        project_root()
            .join(".local/ssh-fixture/remote-root/run/qingzhou-fixture")
            .join("connection-count.state"),
    )
    .unwrap()
    .trim()
    .parse()
    .unwrap()
}

#[tokio::test]
#[ignore = "requires scripts/ssh-fixture.ps1 -Action Start"]
async fn directory_browsing_and_task_execution_share_one_transport() {
    let data_root = project_root()
        .join(".local/test-data")
        .join(format!("m2-session-reuse-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&data_root).unwrap();
    let _cleanup = Cleanup(data_root.clone());
    let services = AppServices::open_with_protector(&data_root, Arc::new(XorProtector))
        .await
        .unwrap();
    let server_id = trusted_server(
        &services,
        CredentialInput::Password {
            password: PASSWORD_CANARY.into(),
        },
        "fixture-session-reuse",
    )
    .await;
    let before = fixture_connection_count();

    services
        .list_remote_directory(&server_id, "/")
        .await
        .unwrap();
    services
        .list_remote_directory(&server_id, "/tmp")
        .await
        .unwrap();
    let mut events = VecEventSink::default();
    let details = services
        .start_task_execution(
            &server_id,
            TaskExecutionRequest {
                task_id: "system.disk_usage".into(),
                parameters: json!({}),
                dangerous_confirmed: false,
            },
            &mut events,
        )
        .await
        .unwrap();

    assert_eq!(details.record.status, ExecutionStatus::Succeeded);
    assert_eq!(fixture_connection_count() - before, 1);
}

#[tokio::test]
#[ignore = "requires scripts/ssh-fixture.ps1 -Action Start"]
async fn tasks_scripts_logs_sftp_history_and_canaries_close_the_m2_loop() {
    let data_root = project_root()
        .join(".local/test-data")
        .join(format!("m2-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&data_root).unwrap();
    let _cleanup = Cleanup(data_root.clone());
    let services = AppServices::open_with_protector(&data_root, Arc::new(XorProtector))
        .await
        .unwrap();

    let password_server = trusted_server(
        &services,
        CredentialInput::Password {
            password: PASSWORD_CANARY.into(),
        },
        "fixture-password",
    )
    .await;
    let private_key =
        std::fs::read_to_string(project_root().join(".local/test-keys/id_ed25519")).unwrap();
    let key_server = trusted_server(
        &services,
        CredentialInput::PrivateKey {
            private_key,
            passphrase: Some(PASSPHRASE_CANARY.into()),
        },
        "fixture-key",
    )
    .await;
    for server_id in [&password_server, &key_server] {
        let capabilities = services.test_connection(server_id).await.unwrap();
        assert_eq!(capabilities.os_id, "ubuntu");
        assert!(capabilities
            .commands
            .iter()
            .any(|command| command == "gzip"));
    }

    let available = services
        .list_task_definitions(&password_server)
        .await
        .unwrap();
    assert!(available.iter().any(|task| {
        task.definition.id == "system.disk_usage"
            && task.state == qingzhou_ssh_lib::core::tasks::TaskAvailabilityState::Ready
    }));
    assert!(available.iter().any(|task| {
        task.definition.id == "service.status"
            && task.state == qingzhou_ssh_lib::core::tasks::TaskAvailabilityState::Ready
    }));

    let mut all_events = Vec::new();
    let mut events = VecEventSink::default();
    let disk = services
        .start_task_execution(
            &password_server,
            TaskExecutionRequest {
                task_id: "system.disk_usage".into(),
                parameters: json!({}),
                dangerous_confirmed: false,
            },
            &mut events,
        )
        .await
        .unwrap();
    assert_eq!(disk.record.status, ExecutionStatus::Succeeded);
    all_events.append(&mut events.events);

    let mut events = VecEventSink::default();
    let service = services
        .start_task_execution(
            &password_server,
            TaskExecutionRequest {
                task_id: "service.status".into(),
                parameters: json!({"service": "qingzhou-fixture.service"}),
                dangerous_confirmed: false,
            },
            &mut events,
        )
        .await
        .unwrap();
    assert_eq!(service.record.status, ExecutionStatus::Succeeded);
    all_events.append(&mut events.events);

    for request in [
        CustomExecutionRequest {
            mode: CustomExecutionMode::Command,
            content: "printf 'advanced-command-ok\\n'".into(),
            timeout_seconds: 30,
            dangerous_confirmed: true,
        },
        CustomExecutionRequest {
            mode: CustomExecutionMode::Script,
            content: format!("printf 'advanced-script-ok\\n'\n# {SCRIPT_CANARY}"),
            timeout_seconds: 30,
            dangerous_confirmed: true,
        },
    ] {
        let mut events = VecEventSink::default();
        let details = services
            .start_custom_execution(&password_server, request, &mut events)
            .await
            .unwrap();
        assert_eq!(details.record.status, ExecutionStatus::Succeeded);
        assert!(details.parameters.iter().any(
            |parameter| parameter.name == "content" && parameter.display_value == "[REDACTED]"
        ));
        all_events.append(&mut events.events);
    }

    for (path, download_name) in [
        ("/var/log/qingzhou.log", "normal-results.txt"),
        ("/var/log/qingzhou.log.gz", "gzip-results.txt"),
    ] {
        let mut events = VecEventSink::default();
        let details = services
            .search_logs(
                &password_server,
                LogSearchRequest {
                    target: LogSearchTarget::Content,
                    path: path.into(),
                    keyword: "ERROR".into(),
                    case_sensitive: true,
                    context_lines: 1,
                    limit: 50,
                    start_time: None,
                    end_time: None,
                },
                &mut events,
            )
            .await
            .unwrap();
        assert_eq!(details.record.status, ExecutionStatus::Succeeded);
        let page = services
            .read_log_result_page(details.record.id, None, 50)
            .await
            .unwrap();
        assert!(page.items.iter().any(|item| matches!(
            item,
            SearchResultItem::Content(item)
                if item.text.contains("ERROR fixture failure")
        )));
        let relative = services
            .download_log_result(details.record.id, download_name)
            .await
            .unwrap();
        assert!(relative.starts_with("downloads/"));
        all_events.append(&mut events.events);
    }

    let upload_source = data_root.join("upload-source.txt");
    tokio::fs::write(&upload_source, include_bytes!("../../tests/m2-canary.txt"))
        .await
        .unwrap();
    let remote_path = format!("/tmp/qingzhou-m2-{}.txt", uuid::Uuid::new_v4());
    let mut upload_events = VecEventSink::default();
    let uploaded = services
        .upload_file(
            &password_server,
            UploadRequest {
                local_path: upload_source,
                remote_path: remote_path.clone(),
                overwrite: false,
                verification: qingzhou_ssh_lib::core::sftp::VerificationPolicy::Balanced,
            },
            &mut upload_events,
        )
        .await
        .unwrap();
    assert_eq!(uploaded.record.status, ExecutionStatus::Succeeded);
    all_events.append(&mut upload_events.events);
    let mut download_events = VecEventSink::default();
    let downloaded = services
        .download_file(
            &password_server,
            DownloadRequest {
                remote_path,
                suggested_name: "m2-roundtrip.txt".into(),
                overwrite: false,
                verification: qingzhou_ssh_lib::core::sftp::VerificationPolicy::Balanced,
            },
            &mut download_events,
        )
        .await
        .unwrap();
    assert_eq!(downloaded.record.status, ExecutionStatus::Succeeded);
    assert_eq!(
        tokio::fs::read(data_root.join("downloads/m2-roundtrip.txt"))
            .await
            .unwrap(),
        include_bytes!("../../tests/m2-canary.txt")
    );
    all_events.append(&mut download_events.events);

    let history = services
        .list_executions(ExecutionFilter::default())
        .await
        .unwrap();
    assert!(history.len() >= 8);
    assert!(history.iter().all(|record| record.status.is_terminal()));
    let event_json = serde_json::to_string(&all_events).unwrap();
    for canary in [PASSWORD_CANARY, PASSPHRASE_CANARY, SCRIPT_CANARY] {
        assert!(!event_json.contains(canary), "event leak: {canary}");
        assert_no_canary_in_files(&data_root, canary);
    }
}

fn assert_no_canary_in_files(root: &Path, canary: &str) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            for entry in std::fs::read_dir(&path).unwrap() {
                pending.push(entry.unwrap().path());
            }
        } else {
            let contents = std::fs::read(&path).unwrap();
            assert!(
                !String::from_utf8_lossy(&contents).contains(canary),
                "file leak in {}",
                path.display()
            );
        }
    }
}
