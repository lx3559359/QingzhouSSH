use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};

use qingzhou_ssh_lib::{
    core::secret_protector::SecretProtector,
    domain::{
        server::{CreateServerRequest, CredentialInput},
        workflow::{
            EqualityOperator, NodePosition, WorkflowCondition, WorkflowCustomMode, WorkflowDraft,
            WorkflowEdge, WorkflowEdgeBranch, WorkflowNode, WorkflowNodeConfig, WorkflowNodeStatus,
            WorkflowRestorePointStatus, WorkflowRunStatus,
        },
        workflow_events::VecWorkflowEventSink,
    },
    error::AppResult,
    services::{app_services::AppServices, workflow_service::StartWorkflowRunRequest},
};
use serde_json::json;
use uuid::Uuid;

struct XorProtector;

impl SecretProtector for XorProtector {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xa5).collect())
    }

    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        Ok(value.iter().map(|byte| byte ^ 0xa5).collect())
    }
}

fn node(id: Uuid, name: &str, config: WorkflowNodeConfig) -> WorkflowNode {
    WorkflowNode {
        id,
        name: name.into(),
        position: NodePosition { x: 0.0, y: 0.0 },
        config,
    }
}

fn edge(from: Uuid, to: Uuid, branch: WorkflowEdgeBranch) -> WorkflowEdge {
    WorkflowEdge { from, to, branch }
}

async fn trusted_services(data_root: &Path) -> (AppServices, String) {
    let services = AppServices::open_with_protector(data_root, Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "workflow live".into(),
            host: "127.0.0.1".into(),
            port: 2222,
            username: "testuser".into(),
            credential: CredentialInput::Password {
                password: "testpass".into(),
            },
        })
        .await
        .unwrap();
    let observation = services
        .inspect_host_key(&server.id)
        .await
        .unwrap()
        .observed;
    services
        .trust_host_key(&server.id, observation)
        .await
        .unwrap();
    (services, server.id)
}

async fn run_single_failure(
    services: &AppServices,
    server_id: &str,
    name: &str,
    config: WorkflowNodeConfig,
) {
    let start = Uuid::new_v4();
    let action = Uuid::new_v4();
    let stop = Uuid::new_v4();
    let definition = services
        .workflow_repository()
        .save(WorkflowDraft {
            id: None,
            name: format!("failure {name}"),
            description: "real fixture failure injection".into(),
            nodes: vec![
                node(start, "start", WorkflowNodeConfig::Start {}),
                node(action, name, config),
                node(
                    stop,
                    "stop",
                    WorkflowNodeConfig::Stop {
                        message: "should not run".into(),
                    },
                ),
            ],
            edges: vec![
                edge(start, action, WorkflowEdgeBranch::Success),
                edge(action, stop, WorkflowEdgeBranch::Success),
            ],
        })
        .await
        .unwrap();
    let mut events = VecWorkflowEventSink::default();
    let details = services
        .workflow_service()
        .run(
            StartWorkflowRunRequest {
                workflow_id: definition.id,
                workflow_version: None,
                server_id: server_id.into(),
                dangerous_confirmed: true,
            },
            &mut events,
        )
        .await
        .unwrap();
    assert_eq!(details.run.status, WorkflowRunStatus::Paused, "{name}");
    assert_eq!(details.run.current_node_id, Some(action), "{name}");
    assert_eq!(
        details
            .node_runs
            .iter()
            .find(|run| run.node_id == action)
            .unwrap()
            .status,
        WorkflowNodeStatus::Failed,
        "{name}"
    );
    assert!(details.node_runs.iter().all(|run| run.node_id != stop));
}

fn files_contain(root: &Path, needle: &[u8]) -> bool {
    if !root.exists() {
        return false;
    }
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            if let Ok(entries) = std::fs::read_dir(path) {
                pending.extend(entries.filter_map(Result::ok).map(|entry| entry.path()));
            }
        } else if std::fs::read(path)
            .is_ok_and(|bytes| bytes.windows(needle.len()).any(|window| window == needle))
        {
            return true;
        }
    }
    false
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires scripts/ssh-fixture.ps1 -Action Start"]
async fn reference_workflow_pauses_retries_branches_rolls_back_and_redacts_diagnostics() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap();
    let data_root = project_root
        .join(".local/test-data")
        .join(format!("workflow-live-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&data_root).unwrap();
    let (services, server_id) = trusted_services(&data_root).await;
    let fixture_root = project_root.join(".local/ssh-fixture/remote-root");
    let remote_file = fixture_root.join("opt/qingzhou-app/config.yml");
    let original = b"version: fixture-original\n";
    std::fs::write(&remote_file, original).unwrap();

    let upload_source = data_root.join("staging/deploy-config.yml");
    std::fs::create_dir_all(upload_source.parent().unwrap()).unwrap();
    let suffix = Uuid::new_v4();
    let download_name = format!("workflow-live-{suffix}.yml");
    let script_canary = "workflow-live-script-sensitive-canary";
    let ids = (0..10).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
    let [start, task, custom, logs, upload, download, condition, true_stop, false_stop, _] =
        ids.as_slice()
    else {
        unreachable!()
    };
    let definition = services
        .workflow_repository()
        .save(WorkflowDraft {
            id: None,
            name: "reference live deployment".into(),
            description: "task, script, logs, transfer, branch and recovery".into(),
            nodes: vec![
                node(*start, "start", WorkflowNodeConfig::Start {}),
                node(
                    *task,
                    "system overview",
                    WorkflowNodeConfig::Task {
                        task_id: "system.overview".into(),
                        task_version: 1,
                        parameters: BTreeMap::new(),
                    },
                ),
                node(
                    *custom,
                    "deploy hook",
                    WorkflowNodeConfig::Custom {
                        mode: WorkflowCustomMode::Script,
                        content: format!("printf advanced-script-ok # {script_canary}"),
                        timeout_seconds: 30,
                    },
                ),
                node(
                    *logs,
                    "health logs",
                    WorkflowNodeConfig::LogSearch {
                        path: "/var/log/qingzhou.log".into(),
                        keyword: "ERROR".into(),
                        case_sensitive: true,
                        context_lines: 1,
                        limit: 20,
                        start_time: None,
                        end_time: None,
                    },
                ),
                node(
                    *upload,
                    "upload config",
                    WorkflowNodeConfig::Upload {
                        local_path: upload_source.to_string_lossy().into_owned(),
                        remote_path: "/opt/qingzhou-app/config.yml".into(),
                        overwrite: true,
                        create_restore_point: true,
                    },
                ),
                node(
                    *download,
                    "download verification",
                    WorkflowNodeConfig::Download {
                        remote_path: "/opt/qingzhou-app/config.yml".into(),
                        suggested_name: download_name.clone(),
                        overwrite: false,
                    },
                ),
                node(
                    *condition,
                    "three log records",
                    WorkflowNodeConfig::Condition {
                        source_node_id: *logs,
                        predicate: WorkflowCondition::ResultField {
                            path: "count".into(),
                            operator: EqualityOperator::Equal,
                            value: json!(3),
                        },
                    },
                ),
                node(
                    *true_stop,
                    "healthy",
                    WorkflowNodeConfig::Stop {
                        message: "deployment complete".into(),
                    },
                ),
                node(
                    *false_stop,
                    "unhealthy",
                    WorkflowNodeConfig::Stop {
                        message: "deployment stopped".into(),
                    },
                ),
            ],
            edges: vec![
                edge(*start, *task, WorkflowEdgeBranch::Success),
                edge(*task, *custom, WorkflowEdgeBranch::Success),
                edge(*custom, *logs, WorkflowEdgeBranch::Success),
                edge(*logs, *upload, WorkflowEdgeBranch::Success),
                edge(*upload, *download, WorkflowEdgeBranch::Success),
                edge(*download, *condition, WorkflowEdgeBranch::Success),
                edge(*condition, *true_stop, WorkflowEdgeBranch::True),
                edge(*condition, *false_stop, WorkflowEdgeBranch::False),
            ],
        })
        .await
        .unwrap();

    let mut first_events = VecWorkflowEventSink::default();
    let paused = services
        .workflow_service()
        .run(
            StartWorkflowRunRequest {
                workflow_id: definition.id,
                workflow_version: Some(definition.version),
                server_id: server_id.clone(),
                dangerous_confirmed: true,
            },
            &mut first_events,
        )
        .await
        .unwrap();
    assert_eq!(paused.run.status, WorkflowRunStatus::Paused);
    assert_eq!(paused.run.current_node_id, Some(*upload));
    assert!(paused.run.retryable);
    assert_eq!(std::fs::read(&remote_file).unwrap(), original);
    assert_eq!(paused.restore_points.len(), 1);

    let deployed = b"version: workflow-deployed\n";
    std::fs::write(&upload_source, deployed).unwrap();
    let mut retry_events = VecWorkflowEventSink::default();
    let completed = services
        .workflow_service()
        .retry_failed_node(paused.run.id, true, &mut retry_events)
        .await
        .unwrap();
    assert_eq!(completed.run.status, WorkflowRunStatus::Succeeded);
    assert_eq!(std::fs::read(&remote_file).unwrap(), deployed);
    assert_eq!(
        std::fs::read(data_root.join("downloads").join(&download_name)).unwrap(),
        deployed
    );
    assert_eq!(
        completed
            .node_runs
            .iter()
            .filter(|run| run.node_id == *upload)
            .count(),
        2
    );
    assert_eq!(
        completed
            .node_runs
            .iter()
            .find(|run| run.node_id == *true_stop)
            .unwrap()
            .status,
        WorkflowNodeStatus::Succeeded
    );
    assert_eq!(
        completed
            .node_runs
            .iter()
            .find(|run| run.node_id == *false_stop)
            .unwrap()
            .status,
        WorkflowNodeStatus::Skipped
    );
    assert!(completed
        .events
        .windows(2)
        .all(|pair| pair[0].sequence < pair[1].sequence));
    assert!(completed.restore_points.iter().all(|point| {
        point.status == WorkflowRestorePointStatus::Available
            && point
                .relative_path
                .as_deref()
                .is_some_and(|path| path.starts_with("backups/workflows/"))
    }));

    let rolled_back = services
        .restore_point_service()
        .rollback_run(completed.run.id, true)
        .await
        .unwrap();
    assert_eq!(rolled_back.run.status, WorkflowRunStatus::RolledBack);
    assert_eq!(std::fs::read(&remote_file).unwrap(), original);

    let diagnostic = services
        .workflow_diagnostics_service()
        .export(completed.run.id)
        .await
        .unwrap();
    assert!(diagnostic.relative_path.starts_with("downloads/"));
    let diagnostic_bytes = std::fs::read(data_root.join(&diagnostic.relative_path)).unwrap();
    let encoded_details = serde_json::to_vec(&completed).unwrap();
    for secret in [script_canary.as_bytes(), b"testpass"] {
        assert!(!diagnostic_bytes
            .windows(secret.len())
            .any(|window| window == secret));
        assert!(!encoded_details
            .windows(secret.len())
            .any(|window| window == secret));
        assert!(!files_contain(&data_root.join("backups"), secret));
        assert!(!files_contain(&data_root.join("downloads"), secret));
    }
    let database = std::fs::read(data_root.join("app.db")).unwrap();
    assert!(!database
        .windows(b"testpass".len())
        .any(|window| window == b"testpass"));

    let cleaned = services
        .restore_point_service()
        .cleanup_run(completed.run.id)
        .await
        .unwrap();
    assert!(cleaned >= 1);

    let mut service_parameters = BTreeMap::new();
    service_parameters.insert("service".into(), json!("workflow-fail.service"));
    run_single_failure(
        &services,
        &server_id,
        "task failure",
        WorkflowNodeConfig::Task {
            task_id: "service.restart".into(),
            task_version: 1,
            parameters: service_parameters,
        },
    )
    .await;
    run_single_failure(
        &services,
        &server_id,
        "script failure",
        WorkflowNodeConfig::Custom {
            mode: WorkflowCustomMode::Script,
            content: "printf workflow-fail-script".into(),
            timeout_seconds: 30,
        },
    )
    .await;
    run_single_failure(
        &services,
        &server_id,
        "log failure",
        WorkflowNodeConfig::LogSearch {
            path: "/var/log/workflow-fail.log".into(),
            keyword: "ERROR".into(),
            case_sensitive: true,
            context_lines: 0,
            limit: 20,
            start_time: None,
            end_time: None,
        },
    )
    .await;
    run_single_failure(
        &services,
        &server_id,
        "upload failure",
        WorkflowNodeConfig::Upload {
            local_path: data_root
                .join("missing-upload.bin")
                .to_string_lossy()
                .into_owned(),
            remote_path: format!("/tmp/missing-upload-{suffix}.bin"),
            overwrite: false,
            create_restore_point: false,
        },
    )
    .await;
    run_single_failure(
        &services,
        &server_id,
        "download failure",
        WorkflowNodeConfig::Download {
            remote_path: format!("/tmp/missing-download-{suffix}.bin"),
            suggested_name: format!("missing-download-{suffix}.bin"),
            overwrite: false,
        },
    )
    .await;

    drop(services);
    let mut removed = false;
    for _ in 0..20 {
        match std::fs::remove_dir_all(&data_root) {
            Ok(()) => {
                removed = true;
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                removed = true;
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    assert!(removed, "workflow live data root remained locked");
}
