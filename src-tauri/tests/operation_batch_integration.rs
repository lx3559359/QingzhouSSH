use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use qingzhou_ssh_lib::{
    core::secret_protector::SecretProtector,
    domain::{
        operation_batch::{OperationBatchItemStatus, OperationBatchRequest, OperationBatchStatus},
        server::{CreateServerRequest, CredentialInput},
    },
    error::AppResult,
    services::{app_services::AppServices, operation_batch_service::BatchItemOutcome},
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

async fn fixture(server_count: usize) -> (tempfile::TempDir, AppServices, Vec<String>) {
    let root = tempfile::tempdir().unwrap();
    let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let mut server_ids = Vec::new();
    for index in 0..server_count {
        let server = services
            .create_server(CreateServerRequest {
                name: format!("batch-server-{index}"),
                host: "127.0.0.1".into(),
                port: 22,
                username: "tester".into(),
                credential: CredentialInput::Password {
                    password: format!("batch-secret-{index}"),
                },
            })
            .await
            .unwrap();
        server_ids.push(server.id);
    }
    (root, services, server_ids)
}

fn update_max(maximum: &AtomicUsize, value: usize) {
    let mut observed = maximum.load(Ordering::SeqCst);
    while value > observed {
        match maximum.compare_exchange(observed, value, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => break,
            Err(actual) => observed = actual,
        }
    }
}

#[tokio::test]
async fn readonly_batch_caps_concurrency_and_isolates_failures() {
    let (_root, services, server_ids) = fixture(5).await;
    let failed_server = server_ids[2].clone();
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let result = services
        .operation_batch_service()
        .start_with_runner(
            OperationBatchRequest {
                server_ids,
                task_id: "system.overview".into(),
                task_version: 2,
                parameters: json!({}),
            },
            {
                let active = active.clone();
                let maximum = maximum.clone();
                move |server_id, _cancel| {
                    let active = active.clone();
                    let maximum = maximum.clone();
                    let failed_server = failed_server.clone();
                    async move {
                        let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                        update_max(&maximum, now);
                        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                        active.fetch_sub(1, Ordering::SeqCst);
                        if server_id == failed_server {
                            BatchItemOutcome::failed("模拟单机失败")
                        } else {
                            BatchItemOutcome::succeeded(None)
                        }
                    }
                }
            },
        )
        .await
        .unwrap();

    assert!(maximum.load(Ordering::SeqCst) <= 3);
    assert_eq!(result.batch.status, OperationBatchStatus::Partial);
    assert_eq!(
        result
            .items
            .iter()
            .filter(|item| item.status == OperationBatchItemStatus::Failed)
            .count(),
        1
    );
    assert_eq!(
        result
            .items
            .iter()
            .filter(|item| item.status == OperationBatchItemStatus::Succeeded)
            .count(),
        4
    );
}

#[tokio::test]
async fn batch_rejects_caution_and_dangerous_tasks_before_creating_rows() {
    let (root, services, server_ids) = fixture(1).await;
    let batch = services.operation_batch_service();

    for task_id in ["network.packet_capture", "service.restart"] {
        let result = batch
            .start_with_runner(
                OperationBatchRequest {
                    server_ids: server_ids.clone(),
                    task_id: task_id.into(),
                    task_version: 2,
                    parameters: json!({}),
                },
                |_server_id, _cancel| async { BatchItemOutcome::succeeded(None) },
            )
            .await;
        assert!(result.is_err(), "{task_id}");
    }

    let database = qingzhou_ssh_lib::core::database::Database::open(root.path())
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM operation_batches")
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn background_batch_returns_an_id_and_cancels_active_children() {
    let (_root, services, server_ids) = fixture(2).await;
    let batch = services.operation_batch_service();
    let started = batch
        .start_background_with_runner(
            OperationBatchRequest {
                server_ids,
                task_id: "system.overview".into(),
                task_version: 2,
                parameters: json!({}),
            },
            |_server_id, cancel| async move {
                cancel.cancelled().await;
                BatchItemOutcome {
                    operation_run_id: None,
                    status: OperationBatchItemStatus::Cancelled,
                    error_message: Some("测试取消".into()),
                }
            },
        )
        .await
        .unwrap();
    batch.cancel(started.batch.id).await.unwrap();

    let finished = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let details = batch.get(started.batch.id).await.unwrap().unwrap();
            if details.batch.status.is_terminal() {
                break details;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(finished.batch.status, OperationBatchStatus::Cancelled);
    assert!(finished
        .items
        .iter()
        .all(|item| item.status == OperationBatchItemStatus::Cancelled));
}
