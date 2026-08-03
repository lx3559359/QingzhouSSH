use std::{collections::BTreeMap, sync::Arc};

use qingzhou_ssh_lib::{
    core::{database::Database, secret_protector::SecretProtector},
    domain::{
        server::{CreateServerRequest, CredentialInput, StoredHostKey},
        workflow::{
            EqualityOperator, NodePosition, WorkflowCondition, WorkflowCustomMode, WorkflowDraft,
            WorkflowEdge, WorkflowEdgeBranch, WorkflowNode, WorkflowNodeConfig, WorkflowNodeStatus,
            WorkflowRunStatus,
        },
        workflow_events::VecWorkflowEventSink,
    },
    error::AppResult,
    repositories::server_repository::ServerRepository,
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

struct BranchIds {
    start: Uuid,
    condition: Uuid,
    true_stop: Uuid,
    false_stop: Uuid,
}

fn branch_workflow(predicate: WorkflowCondition) -> (WorkflowDraft, BranchIds) {
    let ids = BranchIds {
        start: Uuid::new_v4(),
        condition: Uuid::new_v4(),
        true_stop: Uuid::new_v4(),
        false_stop: Uuid::new_v4(),
    };
    let draft = WorkflowDraft {
        id: None,
        name: "branch runner".into(),
        description: "deterministic branch".into(),
        nodes: vec![
            node(ids.start, "start", WorkflowNodeConfig::Start {}),
            node(
                ids.condition,
                "condition",
                WorkflowNodeConfig::Condition {
                    source_node_id: ids.start,
                    predicate,
                },
            ),
            node(
                ids.true_stop,
                "true stop",
                WorkflowNodeConfig::Stop {
                    message: "true".into(),
                },
            ),
            node(
                ids.false_stop,
                "false stop",
                WorkflowNodeConfig::Stop {
                    message: "false".into(),
                },
            ),
        ],
        edges: vec![
            edge(ids.start, ids.condition, WorkflowEdgeBranch::Success),
            edge(ids.condition, ids.true_stop, WorkflowEdgeBranch::True),
            edge(ids.condition, ids.false_stop, WorkflowEdgeBranch::False),
        ],
    };
    (draft, ids)
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

async fn services_with_server(trusted: bool) -> (tempfile::TempDir, AppServices, String) {
    let root = tempfile::tempdir().unwrap();
    let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "runner server".into(),
            host: "127.0.0.1".into(),
            port: 9,
            username: "tester".into(),
            credential: CredentialInput::Password {
                password: "workflow-runner-canary".into(),
            },
        })
        .await
        .unwrap();
    if trusted {
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
    }
    (root, services, server.id)
}

#[tokio::test]
async fn runs_one_condition_branch_and_marks_the_other_branch_skipped() {
    let (_root, services, server_id) = services_with_server(true).await;
    let (draft, ids) = branch_workflow(WorkflowCondition::ExitCode {
        operator: qingzhou_ssh_lib::domain::workflow::NumericOperator::Equal,
        value: 0,
    });
    let definition = services.workflow_repository().save(draft).await.unwrap();
    let mut events = VecWorkflowEventSink::default();

    let details = services
        .workflow_service()
        .run(
            StartWorkflowRunRequest {
                workflow_id: definition.id,
                workflow_version: Some(definition.version),
                server_id,
                dangerous_confirmed: false,
            },
            &mut events,
        )
        .await
        .unwrap();

    assert_eq!(details.run.status, WorkflowRunStatus::Succeeded);
    for id in [ids.start, ids.condition, ids.true_stop] {
        assert_eq!(
            details
                .node_runs
                .iter()
                .find(|node| node.node_id == id)
                .unwrap()
                .status,
            WorkflowNodeStatus::Succeeded
        );
    }
    assert_eq!(
        details
            .node_runs
            .iter()
            .find(|node| node.node_id == ids.false_stop)
            .unwrap()
            .status,
        WorkflowNodeStatus::Skipped
    );
    assert!(details
        .events
        .windows(2)
        .all(|pair| pair[0].sequence < pair[1].sequence));
    assert_eq!(
        events.events.last().unwrap().payload,
        qingzhou_ssh_lib::domain::workflow_events::WorkflowEventPayload::Finished {
            run_id: details.run.id,
            status: WorkflowRunStatus::Succeeded,
            duration_ms: details.run.duration_ms.unwrap(),
        }
    );
}

#[tokio::test]
async fn pauses_when_condition_evaluation_fails_and_leaves_following_nodes_unrun() {
    let (_root, services, server_id) = services_with_server(true).await;
    let (draft, ids) = branch_workflow(WorkflowCondition::ResultField {
        path: "missing.value".into(),
        operator: EqualityOperator::Equal,
        value: json!(true),
    });
    let definition = services.workflow_repository().save(draft).await.unwrap();
    let mut events = VecWorkflowEventSink::default();

    let details = services
        .workflow_service()
        .run(
            StartWorkflowRunRequest {
                workflow_id: definition.id,
                workflow_version: None,
                server_id,
                dangerous_confirmed: false,
            },
            &mut events,
        )
        .await
        .unwrap();

    assert_eq!(details.run.status, WorkflowRunStatus::Paused);
    assert_eq!(details.run.current_node_id, Some(ids.condition));
    assert_eq!(
        details
            .node_runs
            .iter()
            .find(|node| node.node_id == ids.condition)
            .unwrap()
            .status,
        WorkflowNodeStatus::Failed
    );
    assert!(details
        .node_runs
        .iter()
        .all(|node| node.node_id != ids.true_stop && node.node_id != ids.false_stop));
    assert!(!details.run.retryable);
}

#[tokio::test]
async fn preflight_rejects_untrusted_invalid_and_unconfirmed_dangerous_workflows() {
    let (_root, untrusted, server_id) = services_with_server(false).await;
    let (draft, _) = branch_workflow(WorkflowCondition::ExitCode {
        operator: qingzhou_ssh_lib::domain::workflow::NumericOperator::Equal,
        value: 0,
    });
    let definition = untrusted.workflow_repository().save(draft).await.unwrap();
    let mut events = VecWorkflowEventSink::default();
    assert!(untrusted
        .workflow_service()
        .run(
            StartWorkflowRunRequest {
                workflow_id: definition.id,
                workflow_version: None,
                server_id,
                dangerous_confirmed: false,
            },
            &mut events,
        )
        .await
        .is_err());
    assert!(untrusted
        .workflow_repository()
        .list_runs(Default::default())
        .await
        .unwrap()
        .is_empty());

    let (_root, services, server_id) = services_with_server(true).await;
    let start = Uuid::new_v4();
    let custom = Uuid::new_v4();
    let stop = Uuid::new_v4();
    let dangerous = WorkflowDraft {
        id: None,
        name: "dangerous".into(),
        description: String::new(),
        nodes: vec![
            node(start, "start", WorkflowNodeConfig::Start {}),
            node(
                custom,
                "custom",
                WorkflowNodeConfig::Custom {
                    mode: WorkflowCustomMode::Command,
                    content: "uptime".into(),
                    timeout_seconds: 30,
                },
            ),
            node(
                stop,
                "stop",
                WorkflowNodeConfig::Stop {
                    message: "done".into(),
                },
            ),
        ],
        edges: vec![
            edge(start, custom, WorkflowEdgeBranch::Success),
            edge(custom, stop, WorkflowEdgeBranch::Success),
        ],
    };
    let definition = services
        .workflow_repository()
        .save(dangerous)
        .await
        .unwrap();
    assert!(services
        .workflow_service()
        .run(
            StartWorkflowRunRequest {
                workflow_id: definition.id,
                workflow_version: None,
                server_id: server_id.clone(),
                dangerous_confirmed: false,
            },
            &mut events,
        )
        .await
        .is_err());

    let invalid = WorkflowDraft {
        id: None,
        name: "invalid task".into(),
        description: String::new(),
        nodes: vec![node(
            Uuid::new_v4(),
            "task",
            WorkflowNodeConfig::Task {
                task_id: "missing.task".into(),
                task_version: 1,
                parameters: BTreeMap::new(),
            },
        )],
        edges: Vec::new(),
    };
    let invalid = services.workflow_repository().save(invalid).await.unwrap();
    assert!(services
        .workflow_service()
        .run(
            StartWorkflowRunRequest {
                workflow_id: invalid.id,
                workflow_version: None,
                server_id,
                dangerous_confirmed: true,
            },
            &mut events,
        )
        .await
        .is_err());
    assert!(services
        .workflow_repository()
        .list_runs(Default::default())
        .await
        .unwrap()
        .is_empty());
}
