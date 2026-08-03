use qingzhou_ssh_lib::{
    core::database::Database,
    domain::{
        execution::{
            ExecutionFile, ExecutionFilter, ExecutionParameter, ExecutionStatus, FinishExecution,
            NewExecution,
        },
        server::{AuthKind, ServerProfile},
    },
    repositories::{
        execution_repository::ExecutionRepository, server_repository::ServerRepository,
    },
};

async fn harness() -> (tempfile::TempDir, Database, ServerProfile) {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(root.path()).await.unwrap();
    let server = ServerProfile::new(
        "测试服务器",
        "127.0.0.1",
        22,
        "tester",
        AuthKind::Password,
        "credential-1",
    );
    ServerRepository::new(database.pool().clone())
        .insert(&server)
        .await
        .unwrap();
    (root, database, server)
}

#[tokio::test]
async fn persists_transitions_redacted_parameters_and_files() {
    let (_root, database, server) = harness().await;
    let repository = ExecutionRepository::new(database.pool().clone());
    let record = repository
        .create(NewExecution {
            server_id: server.id.clone(),
            task_id: "system.overview".into(),
            task_version: 1,
            category: "system".into(),
            parameters: vec![ExecutionParameter {
                name: "password".into(),
                display_value: "[REDACTED]".into(),
                sensitive: true,
            }],
        })
        .await
        .unwrap();
    assert_eq!(record.status, ExecutionStatus::Queued);

    repository.mark_running(record.id, 100).await.unwrap();
    repository
        .add_file(
            record.id,
            ExecutionFile {
                id: uuid::Uuid::new_v4(),
                relative_path: "logs/executions/result.log".into(),
                purpose: "execution_log".into(),
                size_bytes: 42,
                sha256: "a".repeat(64),
            },
        )
        .await
        .unwrap();
    repository
        .finish(FinishExecution {
            id: record.id,
            status: ExecutionStatus::Succeeded,
            finished_at: 160,
            duration_ms: 60,
            exit_code: Some(0),
            error_category: None,
            error_message: None,
            retryable: false,
            output_summary: Some("ok".into()),
            remote_process_group: None,
        })
        .await
        .unwrap();

    let details = repository.get(record.id).await.unwrap().unwrap();
    assert_eq!(details.record.status, ExecutionStatus::Succeeded);
    assert_eq!(details.parameters[0].display_value, "[REDACTED]");
    assert_eq!(details.files[0].relative_path, "logs/executions/result.log");
    assert_eq!(
        repository
            .list(ExecutionFilter {
                server_id: Some(server.id),
                status: Some(ExecutionStatus::Succeeded),
                ..ExecutionFilter::default()
            })
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn recovers_interrupted_running_records_as_uncertain() {
    let (_root, database, server) = harness().await;
    let repository = ExecutionRepository::new(database.pool().clone());
    let record = repository
        .create(NewExecution {
            server_id: server.id,
            task_id: "advanced.custom".into(),
            task_version: 1,
            category: "advanced".into(),
            parameters: Vec::new(),
        })
        .await
        .unwrap();
    repository.mark_running(record.id, 100).await.unwrap();

    assert_eq!(repository.recover_interrupted().await.unwrap(), 1);
    let recovered = repository.get(record.id).await.unwrap().unwrap();
    assert_eq!(recovered.record.status, ExecutionStatus::Uncertain);
    assert_eq!(
        recovered.record.error_category.as_deref(),
        Some("interrupted")
    );
}

#[tokio::test]
async fn migration_rejects_invalid_execution_status() {
    let (_root, database, server) = harness().await;
    let result = sqlx::query(
        "INSERT INTO executions (id,server_id,task_id,task_version,category,status,created_at,retryable) VALUES (?,?,?,?,?,'invented',?,0)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(server.id)
    .bind("system.overview")
    .bind(1_i64)
    .bind("system")
    .bind(100_i64)
    .execute(database.pool())
    .await;

    assert!(result.is_err());
}
