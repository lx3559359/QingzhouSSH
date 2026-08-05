use std::sync::Arc;

use qingzhou_ssh_lib::{
    core::{
        database::Database, secret_protector::SecretProtector, sftp::sha256_local_file,
        tasks::RiskLevel,
    },
    domain::{
        operation::{NewOperationRun, OperationStatus},
        operation_batch::{NewOperationBatch, OperationBatchItemStatus},
        server::{CreateServerRequest, CredentialInput},
    },
    error::AppResult,
    repositories::{
        operation_batch_repository::OperationBatchRepository,
        operation_repository::OperationRepository,
    },
    services::{app_services::AppServices, operation_report_service::ReportFormat},
};

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
async fn report_is_project_local_hashed_and_redacted() {
    let root = tempfile::tempdir().unwrap();
    let services = AppServices::open_with_protector(root.path(), Arc::new(XorProtector))
        .await
        .unwrap();
    let server = services
        .create_server(CreateServerRequest {
            name: "报告测试服务器".into(),
            host: "127.0.0.1".into(),
            port: 22,
            username: "tester".into(),
            credential: CredentialInput::Password {
                password: "vault-report-secret".into(),
            },
        })
        .await
        .unwrap();
    let database = Database::open(root.path()).await.unwrap();
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
        .transition(run.id, OperationStatus::Preflighting)
        .await
        .unwrap();
    operations
        .transition(run.id, OperationStatus::Running)
        .await
        .unwrap();
    operations
        .set_result(
            run.id,
            &serde_json::json!({
                "status":"warning",
                "summary":"发现测试问题",
                "findings":[{"level":"warning","title":"测试发现","detail":"password=report-canary"}],
                "suggestions":["请复核"],
                "technicalDetails":format!("password=report-canary\ndata_root={}", root.path().display())
            }),
        )
        .await
        .unwrap();
    operations
        .transition(run.id, OperationStatus::Succeeded)
        .await
        .unwrap();

    for format in [ReportFormat::Json, ReportFormat::Txt] {
        let file = services
            .operation_report_service()
            .export_run(run.id, format)
            .await
            .unwrap();
        assert!(file
            .relative_path
            .starts_with("downloads/reports/operation-"));
        let path = root.path().join(&file.relative_path);
        assert_eq!(file.sha256, sha256_local_file(&path).await.unwrap());
        let body = std::fs::read_to_string(path).unwrap();
        assert!(body.contains("[REDACTED]"));
        assert!(!body.contains("report-canary"));
        assert!(!body.contains(root.path().to_string_lossy().as_ref()));
        if format == ReportFormat::Txt {
            for heading in ["结论", "发现", "建议", "技术详情（已脱敏）"] {
                assert!(body.contains(heading));
            }
        }
    }

    let batches = OperationBatchRepository::new(database.pool().clone());
    let batch = batches
        .create(NewOperationBatch {
            task_id: "system.overview".into(),
            task_version: 2,
            server_ids: vec![server.id.clone()],
        })
        .await
        .unwrap();
    batches.mark_running(batch.id).await.unwrap();
    batches
        .mark_item_running(batch.id, &server.id)
        .await
        .unwrap();
    batches
        .finish_item(
            batch.id,
            &server.id,
            OperationBatchItemStatus::Succeeded,
            Some(run.id),
            None,
        )
        .await
        .unwrap();
    batches.complete(batch.id, false).await.unwrap();

    let file = services
        .operation_report_service()
        .export_batch(batch.id, ReportFormat::Json)
        .await
        .unwrap();
    assert!(file.relative_path.starts_with("downloads/reports/batch-"));
    let path = root.path().join(&file.relative_path);
    let body = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["schemaVersion"], 1);
    assert_eq!(parsed["kind"], "operation_batch");
    assert!(!body.contains("report-canary"));
    assert!(!body.contains(root.path().to_string_lossy().as_ref()));
    let leftovers = std::fs::read_dir(root.path().join("downloads/reports"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".partial"))
        .count();
    assert_eq!(leftovers, 0);
}
