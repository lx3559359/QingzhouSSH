use qingzhou_ssh_lib::{
    core::database::Database,
    domain::{
        execution::NewExecution,
        server::{AuthKind, ServerProfile},
        workflow::{
            FinishWorkflowNode, NewWorkflowRun, NodePosition, WorkflowDraft, WorkflowEdge,
            WorkflowEdgeBranch, WorkflowNode, WorkflowNodeConfig, WorkflowNodeStatus,
            WorkflowRunFilter, WorkflowRunStatus,
        },
    },
    repositories::{
        execution_repository::ExecutionRepository, server_repository::ServerRepository,
        workflow_repository::WorkflowRepository,
    },
};
use serde_json::json;
use uuid::Uuid;

async fn harness() -> (tempfile::TempDir, Database, ServerProfile) {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(root.path()).await.unwrap();
    let server = ServerProfile::new(
        "工作流服务器",
        "127.0.0.1",
        22,
        "tester",
        AuthKind::Password,
        "credential-workflow",
    );
    ServerRepository::new(database.pool().clone())
        .insert(&server)
        .await
        .unwrap();
    (root, database, server)
}

fn draft(id: Option<Uuid>, description: &str) -> WorkflowDraft {
    let start = Uuid::new_v4();
    let stop = Uuid::new_v4();
    WorkflowDraft {
        id,
        name: "参考部署".into(),
        description: description.into(),
        nodes: vec![
            WorkflowNode {
                id: start,
                name: "开始".into(),
                position: NodePosition { x: 80.0, y: 120.0 },
                config: WorkflowNodeConfig::Start,
            },
            WorkflowNode {
                id: stop,
                name: "完成".into(),
                position: NodePosition { x: 360.0, y: 120.0 },
                config: WorkflowNodeConfig::Stop {
                    message: "部署完成".into(),
                },
            },
        ],
        edges: vec![WorkflowEdge {
            from: start,
            to: stop,
            branch: WorkflowEdgeBranch::Success,
        }],
    }
}

#[tokio::test]
async fn migration_preserves_m2_and_adds_checked_workflow_tables() {
    let (_root, database, server) = harness().await;
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND (name='workflows' OR name LIKE 'workflow_%') ORDER BY name",
    )
    .fetch_all(database.pool())
    .await
    .unwrap();
    assert_eq!(
        names,
        vec![
            "workflow_node_runs",
            "workflow_restore_points",
            "workflow_run_events",
            "workflow_runs",
            "workflow_versions",
            "workflows",
        ]
    );

    let workflow_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO workflows (id,name,description,current_version,created_at,updated_at) VALUES (?,'checked','',1,100,100)",
    )
    .bind(workflow_id.to_string())
    .execute(database.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO workflow_versions (workflow_id,version,definition_json,checksum_sha256,created_at) VALUES (?,1,'{}',?,100)",
    )
    .bind(workflow_id.to_string())
    .bind("a".repeat(64))
    .execute(database.pool())
    .await
    .unwrap();
    let invalid = sqlx::query(
        "INSERT INTO workflow_runs (id,workflow_id,workflow_version,server_id,status,created_at,retryable) VALUES (?,?,?,?, 'invented',?,0)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(workflow_id.to_string())
    .bind(1_i64)
    .bind(server.id)
    .bind(100_i64)
    .execute(database.pool())
    .await;
    assert!(invalid.is_err());

    let execution_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM executions")
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert_eq!(execution_count, 0);
}

#[tokio::test]
async fn saves_immutable_versions_and_deduplicates_an_identical_definition() {
    let (_root, database, _server) = harness().await;
    let repository = WorkflowRepository::new(database.pool().clone());

    let first = repository.save(draft(None, "v1")).await.unwrap();
    assert_eq!(first.version, 1);
    let unchanged = repository
        .save(WorkflowDraft::from(first.clone()))
        .await
        .unwrap();
    assert_eq!(unchanged.version, 1);

    let mut changed = WorkflowDraft::from(first.clone());
    changed.description = "v2".into();
    let second = repository.save(changed).await.unwrap();
    assert_eq!(second.id, first.id);
    assert_eq!(second.version, 2);
    assert_eq!(
        repository
            .get(first.id, Some(1))
            .await
            .unwrap()
            .unwrap()
            .description,
        "v1"
    );
    assert_eq!(repository.list().await.unwrap()[0].current_version, 2);
}

#[tokio::test]
async fn persists_run_node_attempts_monotonic_events_and_filters() {
    let (_root, database, server) = harness().await;
    let repository = WorkflowRepository::new(database.pool().clone());
    let workflow = repository.save(draft(None, "run")).await.unwrap();
    let start_node = workflow.nodes[0].id;
    let run = repository
        .create_run(NewWorkflowRun {
            workflow_id: workflow.id,
            workflow_version: workflow.version,
            server_id: server.id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(run.status, WorkflowRunStatus::Queued);

    repository
        .mark_run_running(run.id, start_node, 100)
        .await
        .unwrap();
    let first_attempt = repository
        .start_node_attempt(run.id, start_node, 110)
        .await
        .unwrap();
    assert_eq!(first_attempt.attempt, 1);
    let execution_id = ExecutionRepository::new(database.pool().clone())
        .create(NewExecution {
            server_id: server.id.clone(),
            task_id: "system.overview".into(),
            task_version: 1,
            category: "system".into(),
            parameters: Vec::new(),
        })
        .await
        .unwrap()
        .id;
    repository
        .link_node_execution(run.id, start_node, 1, execution_id)
        .await
        .unwrap();
    repository
        .finish_node(FinishWorkflowNode {
            run_id: run.id,
            node_id: start_node,
            attempt: 1,
            status: WorkflowNodeStatus::Succeeded,
            finished_at: 120,
            exit_code: Some(0),
            output_summary: Some("ok".into()),
            error_message: None,
            retryable: false,
        })
        .await
        .unwrap();

    let first_event = repository
        .append_event(run.id, "run_started", json!({"nodeId": start_node}), 100)
        .await
        .unwrap();
    let second_event = repository
        .append_event(run.id, "node_finished", json!({"status": "succeeded"}), 120)
        .await
        .unwrap();
    assert_eq!((first_event.sequence, second_event.sequence), (1, 2));

    let details = repository.get_run(run.id).await.unwrap().unwrap();
    assert_eq!(details.node_runs[0].execution_id, Some(execution_id));
    assert_eq!(details.events.len(), 2);
    assert_eq!(
        repository
            .list_runs(WorkflowRunFilter {
                server_id: Some(server.id),
                status: Some(WorkflowRunStatus::Running),
                ..WorkflowRunFilter::default()
            })
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn recovers_interrupted_runs_and_nodes_as_uncertain() {
    let (_root, database, server) = harness().await;
    let repository = WorkflowRepository::new(database.pool().clone());
    let workflow = repository.save(draft(None, "recovery")).await.unwrap();
    let node_id = workflow.nodes[0].id;
    let run = repository
        .create_run(NewWorkflowRun {
            workflow_id: workflow.id,
            workflow_version: workflow.version,
            server_id: server.id,
        })
        .await
        .unwrap();
    repository
        .mark_run_running(run.id, node_id, 100)
        .await
        .unwrap();
    repository
        .start_node_attempt(run.id, node_id, 110)
        .await
        .unwrap();

    assert_eq!(repository.recover_interrupted().await.unwrap(), (1, 1));
    let recovered = repository.get_run(run.id).await.unwrap().unwrap();
    assert_eq!(recovered.run.status, WorkflowRunStatus::Uncertain);
    assert_eq!(recovered.node_runs[0].status, WorkflowNodeStatus::Uncertain);
    assert_eq!(recovered.run.current_node_id, Some(node_id));
    assert_eq!(recovered.run.error_category.as_deref(), Some("interrupted"));
}
