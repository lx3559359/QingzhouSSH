use std::{
    path::Path,
    sync::{Arc, Barrier},
    time::Duration,
};

use qingzhou_ssh_lib::{
    core::{database::Database, secret_protector::SecretProtector},
    domain::{
        execution::now_millis,
        server::{CreateServerRequest, CredentialInput, StoredHostKey},
        workflow::{
            FinishWorkflowNode, FinishWorkflowRun, NewWorkflowRun, NodePosition, NumericOperator,
            WorkflowCondition, WorkflowCustomMode, WorkflowDraft, WorkflowEdge, WorkflowEdgeBranch,
            WorkflowNode, WorkflowNodeConfig, WorkflowNodeStatus, WorkflowRunStatus,
        },
        workflow_events::{
            VecWorkflowEventSink, WorkflowEvent, WorkflowEventPayload, WorkflowEventSink,
        },
    },
    error::{AppError, AppResult},
    repositories::server_repository::ServerRepository,
    services::{app_services::AppServices, workflow_service::StartWorkflowRunRequest},
};
use serde_json::json;
use tokio::sync::oneshot;
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

#[derive(Clone, Copy)]
struct Ids {
    start: Uuid,
    condition: Uuid,
    yes: Uuid,
    no: Uuid,
}

fn workflow() -> (WorkflowDraft, Ids) {
    let ids = Ids {
        start: Uuid::new_v4(),
        condition: Uuid::new_v4(),
        yes: Uuid::new_v4(),
        no: Uuid::new_v4(),
    };
    let node = |id, name: &str, config| WorkflowNode {
        id,
        name: name.into(),
        position: NodePosition { x: 0.0, y: 0.0 },
        config,
    };
    (
        WorkflowDraft {
            id: None,
            name: "recovery workflow".into(),
            description: String::new(),
            nodes: vec![
                node(ids.start, "start", WorkflowNodeConfig::Start {}),
                node(
                    ids.condition,
                    "condition",
                    WorkflowNodeConfig::Condition {
                        source_node_id: ids.start,
                        predicate: WorkflowCondition::ExitCode {
                            operator: NumericOperator::Equal,
                            value: 0,
                        },
                    },
                ),
                node(
                    ids.yes,
                    "yes",
                    WorkflowNodeConfig::Stop {
                        message: "yes".into(),
                    },
                ),
                node(
                    ids.no,
                    "no",
                    WorkflowNodeConfig::Stop {
                        message: "no".into(),
                    },
                ),
            ],
            edges: vec![
                WorkflowEdge {
                    from: ids.start,
                    to: ids.condition,
                    branch: WorkflowEdgeBranch::Success,
                },
                WorkflowEdge {
                    from: ids.condition,
                    to: ids.yes,
                    branch: WorkflowEdgeBranch::True,
                },
                WorkflowEdge {
                    from: ids.condition,
                    to: ids.no,
                    branch: WorkflowEdgeBranch::False,
                },
            ],
        },
        ids,
    )
}

async fn services() -> (tempfile::TempDir, AppServices, String) {
    let root = tempfile::tempdir().unwrap();
    let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "recovery server".into(),
            host: "127.0.0.1".into(),
            port: 9,
            username: "tester".into(),
            credential: CredentialInput::Password {
                password: "recovery-canary".into(),
            },
        })
        .await
        .unwrap();
    let database = Database::open(root.path()).await.unwrap();
    ServerRepository::new(database.pool().clone())
        .upsert_host_key(&StoredHostKey {
            server_id: server.id.clone(),
            algorithm: "ssh-ed25519".into(),
            fingerprint_sha256: "SHA256:fixture".into(),
            raw_key_base64: "fixture".into(),
        })
        .await
        .unwrap();
    (root, services, server.id)
}

struct BlockingSink {
    sender: Option<oneshot::Sender<Uuid>>,
    barrier: Arc<Barrier>,
}

struct RunIdSink(Option<oneshot::Sender<Uuid>>);

impl WorkflowEventSink for RunIdSink {
    fn send(&mut self, event: WorkflowEvent) -> AppResult<()> {
        if let WorkflowEventPayload::RunStarted { run_id, .. } = event.payload {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(run_id);
            }
        }
        Ok(())
    }
}

impl WorkflowEventSink for BlockingSink {
    fn send(&mut self, event: WorkflowEvent) -> AppResult<()> {
        if let WorkflowEventPayload::RunStarted { run_id, .. } = event.payload {
            if let Some(sender) = self.sender.take() {
                let _ = sender.send(run_id);
                self.barrier.wait();
            }
        }
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_between_nodes_is_confirmed_and_registry_is_cleaned() {
    let (_root, services, server_id) = services().await;
    let definition = services
        .workflow_repository()
        .save(workflow().0)
        .await
        .unwrap();
    let service = services.workflow_service();
    let cancellation_service = service.clone();
    let barrier = Arc::new(Barrier::new(2));
    let release = barrier.clone();
    let (sender, receiver) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let mut sink = BlockingSink {
            sender: Some(sender),
            barrier,
        };
        service
            .run(
                StartWorkflowRunRequest {
                    workflow_id: definition.id,
                    workflow_version: None,
                    server_id,
                    dangerous_confirmed: false,
                },
                &mut sink,
            )
            .await
    });
    let run_id = receiver.await.unwrap();
    cancellation_service.cancel(run_id).await.unwrap();
    tokio::task::spawn_blocking(move || release.wait())
        .await
        .unwrap();
    let details = handle.await.unwrap().unwrap();

    assert_eq!(details.run.status, WorkflowRunStatus::Cancelled);
    assert!(details
        .node_runs
        .iter()
        .any(|node| node.status == WorkflowNodeStatus::Cancelled));
    assert!(cancellation_service.cancel(run_id).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires scripts/ssh-fixture.ps1 -Action Start"]
async fn cancellation_reaches_the_active_child_execution() {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .unwrap();
    let data_root = project_root
        .join(".local/test-data")
        .join(format!("workflow-cancel-live-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&data_root).unwrap();
    let services = AppServices::open_with_protector(&data_root, Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "cancel live".into(),
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
    let start = Uuid::new_v4();
    let custom = Uuid::new_v4();
    let stop = Uuid::new_v4();
    let definition = services
        .workflow_repository()
        .save(WorkflowDraft {
            id: None,
            name: "cancel child".into(),
            description: String::new(),
            nodes: vec![
                WorkflowNode {
                    id: start,
                    name: "start".into(),
                    position: NodePosition { x: 0.0, y: 0.0 },
                    config: WorkflowNodeConfig::Start {},
                },
                WorkflowNode {
                    id: custom,
                    name: "delayed child".into(),
                    position: NodePosition { x: 100.0, y: 0.0 },
                    config: WorkflowNodeConfig::Custom {
                        mode: WorkflowCustomMode::Command,
                        content: "printf workflow-cancel-delay".into(),
                        timeout_seconds: 30,
                    },
                },
                WorkflowNode {
                    id: stop,
                    name: "stop".into(),
                    position: NodePosition { x: 200.0, y: 0.0 },
                    config: WorkflowNodeConfig::Stop {
                        message: "done".into(),
                    },
                },
            ],
            edges: vec![
                WorkflowEdge {
                    from: start,
                    to: custom,
                    branch: WorkflowEdgeBranch::Success,
                },
                WorkflowEdge {
                    from: custom,
                    to: stop,
                    branch: WorkflowEdgeBranch::Success,
                },
            ],
        })
        .await
        .unwrap();
    let service = services.workflow_service();
    let runner = service.clone();
    let server_id = server.id.clone();
    let (sender, receiver) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let mut sink = RunIdSink(Some(sender));
        runner
            .run(
                StartWorkflowRunRequest {
                    workflow_id: definition.id,
                    workflow_version: None,
                    server_id,
                    dangerous_confirmed: true,
                },
                &mut sink,
            )
            .await
    });
    let run_id = receiver.await.unwrap();
    let child_id = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(child) = service.current_child(run_id).await {
                break child;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    service.cancel(run_id).await.unwrap();
    let details = handle.await.unwrap().unwrap();

    assert_eq!(details.run.status, WorkflowRunStatus::Cancelled);
    assert_eq!(
        details
            .node_runs
            .iter()
            .find(|node| node.node_id == custom)
            .unwrap()
            .execution_id,
        Some(child_id)
    );
    assert!(service.current_child(run_id).await.is_none());
    assert!(service.cancel(run_id).await.is_err());
    drop(service);
    drop(services);
    std::fs::remove_dir_all(data_root).unwrap();
}

async fn seed_paused_run(services: &AppServices, server_id: &str, retryable: bool) -> (Uuid, Ids) {
    let (draft, ids) = workflow();
    let repository = services.workflow_repository();
    let definition = repository.save(draft).await.unwrap();
    let run = repository
        .create_run(NewWorkflowRun {
            workflow_id: definition.id,
            workflow_version: definition.version,
            server_id: server_id.into(),
        })
        .await
        .unwrap();
    repository
        .mark_run_running(run.id, ids.start, now_millis())
        .await
        .unwrap();
    let start = repository
        .start_node_attempt(run.id, ids.start, now_millis())
        .await
        .unwrap();
    repository
        .finish_node(FinishWorkflowNode {
            run_id: run.id,
            node_id: ids.start,
            attempt: start.attempt,
            status: WorkflowNodeStatus::Succeeded,
            finished_at: now_millis(),
            exit_code: Some(0),
            result: Some(json!({"started": true})),
            output_summary: Some("started".into()),
            error_message: None,
            retryable: false,
        })
        .await
        .unwrap();
    repository
        .set_current_node(run.id, ids.condition)
        .await
        .unwrap();
    let failed = repository
        .start_node_attempt(run.id, ids.condition, now_millis())
        .await
        .unwrap();
    repository
        .finish_node(FinishWorkflowNode {
            run_id: run.id,
            node_id: ids.condition,
            attempt: failed.attempt,
            status: WorkflowNodeStatus::Failed,
            finished_at: now_millis(),
            exit_code: None,
            result: None,
            output_summary: None,
            error_message: Some("injected transient failure".into()),
            retryable,
        })
        .await
        .unwrap();
    repository
        .finish_run(FinishWorkflowRun {
            run_id: run.id,
            status: WorkflowRunStatus::Paused,
            finished_at: now_millis(),
            error_category: Some("injected".into()),
            error_message: Some("injected transient failure".into()),
            retryable,
        })
        .await
        .unwrap();
    (run.id, ids)
}

#[tokio::test]
async fn retry_reuses_run_increments_attempt_and_continues_from_failed_node() {
    let (_root, services, server_id) = services().await;
    let (run_id, ids) = seed_paused_run(&services, &server_id, true).await;
    let mut events = VecWorkflowEventSink::default();
    let details = services
        .workflow_service()
        .retry_failed_node(run_id, false, &mut events)
        .await
        .unwrap();

    assert_eq!(details.run.id, run_id);
    assert_eq!(details.run.status, WorkflowRunStatus::Succeeded);
    let attempts = details
        .node_runs
        .iter()
        .filter(|node| node.node_id == ids.condition)
        .collect::<Vec<_>>();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].status, WorkflowNodeStatus::Failed);
    assert_eq!(attempts[1].attempt, 2);
    assert_eq!(attempts[1].status, WorkflowNodeStatus::Succeeded);
    assert_eq!(
        details
            .node_runs
            .iter()
            .find(|node| node.node_id == ids.no)
            .unwrap()
            .status,
        WorkflowNodeStatus::Skipped
    );

    let (non_retryable, _) = seed_paused_run(&services, &server_id, false).await;
    assert!(services
        .workflow_service()
        .retry_failed_node(non_retryable, false, &mut events)
        .await
        .is_err());
}

#[tokio::test]
async fn reopening_marks_running_run_and_node_uncertain_without_continuing() {
    let (root, services, server_id) = services().await;
    let (draft, ids) = workflow();
    let repository = services.workflow_repository();
    let definition = repository.save(draft).await.unwrap();
    let run = repository
        .create_run(NewWorkflowRun {
            workflow_id: definition.id,
            workflow_version: definition.version,
            server_id,
        })
        .await
        .unwrap();
    repository
        .mark_run_running(run.id, ids.condition, now_millis())
        .await
        .unwrap();
    repository
        .start_node_attempt(run.id, ids.condition, now_millis())
        .await
        .unwrap();
    drop(services);

    let reopened = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let details = reopened
        .workflow_repository()
        .get_run(run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(details.run.status, WorkflowRunStatus::Uncertain);
    assert_eq!(details.node_runs[0].status, WorkflowNodeStatus::Uncertain);
    assert_eq!(details.node_runs.len(), 1);
    assert_eq!(details.run.current_node_id, Some(ids.condition));
    assert!(reopened.workflow_service().cancel(run.id).await.is_err());
    assert!(!matches!(
        details.run.status,
        WorkflowRunStatus::Cancelled | WorkflowRunStatus::Succeeded
    ));
}

#[test]
fn cancellation_error_remains_distinct_from_uncertain_remote_state() {
    assert_ne!(
        AppError::Cancelled.code(),
        AppError::RemoteStateUncertain("x".into()).code()
    );
}
