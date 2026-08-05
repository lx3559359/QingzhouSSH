use qingzhou_ssh_lib::{
    core::{database::Database, tasks::RiskLevel},
    domain::{
        operation::NewOperationRun,
        script::{
            NewPersonalScript, NewScriptRunReference, NewScriptVersion, ScriptListFilter,
            ScriptMetadataUpdate,
        },
        server::{AuthKind, ServerProfile},
    },
    repositories::{
        operation_repository::OperationRepository, script_repository::ScriptRepository,
        server_repository::ServerRepository,
    },
};
use serde_json::json;

async fn harness() -> (tempfile::TempDir, Database, ScriptRepository) {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(root.path()).await.unwrap();
    let repository = ScriptRepository::new(database.pool().clone());
    (root, database, repository)
}

fn new_script(title: &str, category: &str, body: &str) -> NewPersonalScript {
    NewPersonalScript {
        title: title.into(),
        category: category.into(),
        tags: vec!["巡检".into(), "个人".into()],
        is_favorite: false,
        is_enabled: true,
        version: NewScriptVersion {
            body: body.into(),
            parameters: json!([]),
            scan_summary: json!({"warningCount":0}),
        },
    }
}

#[tokio::test]
async fn migration_creates_checked_immutable_script_tables() {
    let (_root, database, _repository) = harness().await;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'script_%' ORDER BY name",
    )
    .fetch_all(database.pool())
    .await
    .unwrap();
    assert_eq!(
        tables,
        vec!["script_definitions", "script_runs", "script_versions"]
    );

    let triggers: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='trigger' AND tbl_name='script_versions' ORDER BY name",
    )
    .fetch_all(database.pool())
    .await
    .unwrap();
    assert_eq!(
        triggers,
        vec![
            "script_versions_forbid_delete",
            "script_versions_forbid_update"
        ]
    );
}

#[tokio::test]
async fn saving_changes_creates_immutable_version_and_soft_delete_preserves_history() {
    let (_root, database, repository) = harness().await;
    let created = repository
        .create(new_script("服务器巡检", "系统", "echo one"))
        .await
        .unwrap();
    assert_eq!(created.active_version.version_number, 1);
    assert_eq!(created.active_version.body, "echo one");

    let v2 = repository
        .save_version(
            created.definition.id,
            NewScriptVersion {
                body: "echo two".into(),
                parameters: json!([]),
                scan_summary: json!({"warningCount":0}),
            },
        )
        .await
        .unwrap();
    assert_eq!(v2.version_number, 2);
    assert_eq!(
        repository
            .get_version(created.definition.id, 1)
            .await
            .unwrap()
            .body,
        "echo one"
    );
    assert_eq!(
        repository
            .get_for_editor(created.definition.id)
            .await
            .unwrap()
            .unwrap()
            .active_version
            .body,
        "echo two"
    );

    let update = sqlx::query("UPDATE script_versions SET body='tampered' WHERE id=?")
        .bind(v2.id.to_string())
        .execute(database.pool())
        .await;
    assert!(update.is_err());
    let delete = sqlx::query("DELETE FROM script_versions WHERE id=?")
        .bind(v2.id.to_string())
        .execute(database.pool())
        .await;
    assert!(delete.is_err());

    repository.soft_delete(created.definition.id).await.unwrap();
    assert!(repository
        .get_for_editor(created.definition.id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        repository
            .get_version(created.definition.id, 1)
            .await
            .unwrap()
            .body,
        "echo one"
    );
}

#[tokio::test]
async fn list_filters_metadata_without_returning_script_bodies() {
    let (_root, _database, repository) = harness().await;
    let first = repository
        .create(new_script("数据库巡检", "数据库", "db-body-canary"))
        .await
        .unwrap();
    repository
        .update_metadata(
            first.definition.id,
            ScriptMetadataUpdate {
                title: "数据库巡检".into(),
                category: "数据库".into(),
                tags: vec!["巡检".into(), "数据库".into()],
            },
        )
        .await
        .unwrap();
    repository
        .set_favorite(first.definition.id, true)
        .await
        .unwrap();
    repository
        .create(new_script("系统巡检", "系统", "system-body-canary"))
        .await
        .unwrap();

    let rows = repository
        .list(ScriptListFilter {
            query: Some("数据".into()),
            category: Some("数据库".into()),
            tag: Some("巡检".into()),
            favorite: Some(true),
            enabled: Some(true),
        })
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "数据库巡检");
    let serialized = serde_json::to_string(&rows).unwrap();
    assert!(!serialized.contains("db-body-canary"));
    assert!(!serialized.contains("system-body-canary"));
}

#[tokio::test]
async fn run_reference_remains_after_definition_is_soft_deleted() {
    let (_root, database, repository) = harness().await;
    let server = ServerProfile::new(
        "脚本测试服务器",
        "127.0.0.1",
        22,
        "tester",
        AuthKind::Password,
        "script-credential",
    );
    ServerRepository::new(database.pool().clone())
        .insert(&server)
        .await
        .unwrap();
    let operation = OperationRepository::new(database.pool().clone())
        .create(NewOperationRun {
            server_id: server.id,
            task_id: "script.personal".into(),
            task_version: 1,
            risk_level: RiskLevel::Dangerous,
            parameters_summary: None,
        })
        .await
        .unwrap();
    let script = repository
        .create(new_script("历史脚本", "系统", "history-body-canary"))
        .await
        .unwrap();
    let run = repository
        .record_run(NewScriptRunReference {
            definition_id: script.definition.id,
            version_id: script.active_version.id,
            operation_run_id: operation.id,
        })
        .await
        .unwrap();

    repository.soft_delete(script.definition.id).await.unwrap();
    let persisted = repository.get_run(run.id).await.unwrap().unwrap();
    assert_eq!(persisted.version_id, script.active_version.id);
    assert_eq!(
        repository
            .get_version(script.definition.id, 1)
            .await
            .unwrap()
            .body,
        "history-body-canary"
    );
}
