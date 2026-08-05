use qingzhou_ssh_lib::{
    core::{database::Database, tasks::RiskLevel},
    domain::{
        execution::NewExecution,
        operation::{
            FinishOperationStep, NewOperationRun, NewOperationStep, OperationPhase,
            OperationStatus, OperationStepStatus,
        },
        server::{AuthKind, ServerProfile},
    },
    repositories::{
        execution_repository::ExecutionRepository, operation_repository::OperationRepository,
        server_repository::ServerRepository,
    },
};

async fn harness() -> (tempfile::TempDir, Database, ServerProfile) {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(root.path()).await.unwrap();
    let server = ServerProfile::new(
        "运维测试服务器",
        "127.0.0.1",
        22,
        "tester",
        AuthKind::Password,
        "operation-credential",
    );
    ServerRepository::new(database.pool().clone())
        .insert(&server)
        .await
        .unwrap();
    (root, database, server)
}

#[tokio::test]
async fn operation_state_is_strict_and_interrupted_work_is_uncertain() {
    let (_root, database, server) = harness().await;
    let repo = OperationRepository::new(database.pool().clone());
    let run = repo
        .create(NewOperationRun {
            server_id: server.id,
            task_id: "system.overview".into(),
            task_version: 2,
            risk_level: RiskLevel::Safe,
            parameters_summary: None,
        })
        .await
        .unwrap();
    repo.create_step(NewOperationStep {
        run_id: run.id,
        phase: OperationPhase::Execute,
        step_index: 0,
        step_id: "execute".into(),
        title: "执行任务".into(),
    })
    .await
    .unwrap();
    repo.mark_step_running(run.id, OperationPhase::Execute, 0, 100)
        .await
        .unwrap();
    repo.transition(run.id, OperationStatus::Preflighting)
        .await
        .unwrap();
    repo.transition(run.id, OperationStatus::Running)
        .await
        .unwrap();
    assert!(repo
        .transition(run.id, OperationStatus::PreviewReady)
        .await
        .is_err());
    assert_eq!(repo.recover_interrupted().await.unwrap(), 1);
    let recovered = repo.get(run.id).await.unwrap().unwrap();
    assert_eq!(recovered.run.status, OperationStatus::Uncertain);
    assert_eq!(recovered.steps[0].status, OperationStepStatus::Uncertain);
}

#[tokio::test]
async fn operation_steps_reference_existing_executions() {
    let (_root, database, server) = harness().await;
    let operations = OperationRepository::new(database.pool().clone());
    let run = operations
        .create(NewOperationRun {
            server_id: server.id.clone(),
            task_id: "system.overview".into(),
            task_version: 2,
            risk_level: RiskLevel::Safe,
            parameters_summary: None,
        })
        .await
        .unwrap();
    operations
        .create_step(NewOperationStep {
            run_id: run.id,
            phase: OperationPhase::Execute,
            step_index: 0,
            step_id: "execute".into(),
            title: "执行任务".into(),
        })
        .await
        .unwrap();
    operations
        .mark_step_running(run.id, OperationPhase::Execute, 0, 100)
        .await
        .unwrap();

    let execution = ExecutionRepository::new(database.pool().clone())
        .create(NewExecution {
            server_id: server.id,
            task_id: "system.overview".into(),
            task_version: 2,
            category: "system".into(),
            parameters: Vec::new(),
        })
        .await
        .unwrap();
    operations
        .finish_step(FinishOperationStep {
            run_id: run.id,
            phase: OperationPhase::Execute,
            step_index: 0,
            status: OperationStepStatus::Succeeded,
            execution_id: Some(execution.id),
            output_summary: Some("完成".into()),
            error_message: None,
            finished_at: 150,
        })
        .await
        .unwrap();

    let details = operations.get(run.id).await.unwrap().unwrap();
    assert_eq!(details.steps.len(), 1);
    assert_eq!(details.steps[0].execution_id, Some(execution.id));
    assert_eq!(details.steps[0].status, OperationStepStatus::Succeeded);
}

#[tokio::test]
async fn migration_rejects_unknown_operation_status() {
    let (_root, database, server) = harness().await;
    let result = sqlx::query(
        "INSERT INTO operation_runs (id,server_id,task_id,task_version,risk_level,status,created_at,updated_at) VALUES (?,?,?,?,?,'invented',?,?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(server.id)
    .bind("system.overview")
    .bind(2_i64)
    .bind("safe")
    .bind(100_i64)
    .bind(100_i64)
    .execute(database.pool())
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn structured_operation_result_is_persisted_only_for_active_run() {
    let (_root, database, server) = harness().await;
    let repo = OperationRepository::new(database.pool().clone());
    let run = repo
        .create(NewOperationRun {
            server_id: server.id,
            task_id: "system.overview".into(),
            task_version: 2,
            risk_level: RiskLevel::Safe,
            parameters_summary: None,
        })
        .await
        .unwrap();
    repo.transition(run.id, OperationStatus::Preflighting)
        .await
        .unwrap();
    repo.transition(run.id, OperationStatus::Running)
        .await
        .unwrap();
    repo.set_result(
        run.id,
        &serde_json::json!({"status":"normal","summary":"检查完成"}),
    )
    .await
    .unwrap();
    let details = repo.get(run.id).await.unwrap().unwrap();
    assert_eq!(details.run.result.unwrap()["status"], "normal");

    repo.transition(run.id, OperationStatus::Succeeded)
        .await
        .unwrap();
    assert!(repo
        .set_result(run.id, &serde_json::json!({"status":"warning"}))
        .await
        .is_err());
}

#[tokio::test]
async fn failed_multistep_run_marks_remaining_steps_skipped() {
    let (_root, database, server) = harness().await;
    let repo = OperationRepository::new(database.pool().clone());
    let run = repo
        .create(NewOperationRun {
            server_id: server.id,
            task_id: "runbook.cpu.incident".into(),
            task_version: 2,
            risk_level: RiskLevel::Safe,
            parameters_summary: None,
        })
        .await
        .unwrap();
    for index in 0..3 {
        repo.create_step(NewOperationStep {
            run_id: run.id,
            phase: OperationPhase::Execute,
            step_index: index,
            step_id: format!("step-{index}"),
            title: format!("步骤 {index}"),
        })
        .await
        .unwrap();
    }
    repo.skip_pending_steps(run.id, OperationPhase::Execute, 2)
        .await
        .unwrap();
    let details = repo.get(run.id).await.unwrap().unwrap();
    assert_eq!(details.steps[0].status, OperationStepStatus::Pending);
    assert_eq!(details.steps[1].status, OperationStepStatus::Pending);
    assert_eq!(details.steps[2].status, OperationStepStatus::Skipped);
}
