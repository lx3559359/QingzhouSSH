use qingzhou_ssh_lib::core::database::Database;
use sha2::{Digest, Sha384};
use sqlx::Row;

#[tokio::test]
async fn opens_existing_database_when_migration_differs_only_by_line_endings() {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(root.path()).await.unwrap();
    sqlx::query(
        "INSERT INTO servers (id, name, host, port, username, auth_kind, credential_id) \
         VALUES ('server-1', 'existing server', '127.0.0.1', 22, 'root', 'password', 'credential-1')",
    )
    .execute(database.pool())
    .await
    .unwrap();

    let current_sql = include_str!("../migrations/0001_foundation.sql");
    let alternate_sql = if current_sql.contains("\r\n") {
        current_sql.replace("\r\n", "\n")
    } else {
        current_sql.replace('\n', "\r\n")
    };
    let alternate_checksum = Sha384::digest(alternate_sql.as_bytes()).to_vec();
    sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 1")
        .bind(alternate_checksum)
        .execute(database.pool())
        .await
        .unwrap();
    database.pool().close().await;
    drop(database);

    let reopened = Database::open(root.path()).await.unwrap();
    let row = sqlx::query("SELECT name FROM servers WHERE id = 'server-1'")
        .fetch_one(reopened.pool())
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("name"), "existing server");

    let stored_checksum: Vec<u8> =
        sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = 1")
            .fetch_one(reopened.pool())
            .await
            .unwrap();
    assert_eq!(
        stored_checksum,
        Sha384::digest(current_sql.as_bytes()).to_vec()
    );
}

#[tokio::test]
async fn rejects_a_migration_checksum_that_is_not_a_line_ending_variant() {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open(root.path()).await.unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 1")
        .bind(vec![0x5a_u8; 48])
        .execute(database.pool())
        .await
        .unwrap();
    database.pool().close().await;
    drop(database);

    assert!(Database::open(root.path()).await.is_err());
}
