use std::path::Path;

use sha2::{Digest, Sha384};
use sqlx::{
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool, Transaction,
};

use crate::error::{AppError, AppResult};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn open(data_root: &Path) -> AppResult<Self> {
        let options = SqliteConnectOptions::new()
            .filename(data_root.join("app.db"))
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        repair_line_ending_checksums(&pool).await?;
        MIGRATOR.run(&pool).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

async fn repair_line_ending_checksums(pool: &SqlitePool) -> AppResult<()> {
    let migrations_table_exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_optional(pool)
    .await?;
    if migrations_table_exists.is_none() {
        return Ok(());
    }

    let mut transaction = pool.begin().await?;
    for migration in MIGRATOR.iter() {
        let applied: Option<(String, i64, Vec<u8>)> = sqlx::query_as(
            "SELECT description, success, checksum FROM _sqlx_migrations WHERE version = ?",
        )
        .bind(migration.version)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((description, success, stored_checksum)) = applied else {
            continue;
        };
        if success != 1
            || description != migration.description.as_ref()
            || stored_checksum == migration.checksum.as_ref()
        {
            continue;
        }

        let alternate_sql = alternate_line_endings(migration.sql.as_str());
        let Some(alternate_sql) = alternate_sql else {
            continue;
        };
        let alternate_checksum = Sha384::digest(alternate_sql.as_bytes());
        if stored_checksum.as_slice() != &alternate_checksum[..] {
            continue;
        }

        update_checksum(
            &mut transaction,
            migration.version,
            &stored_checksum,
            migration.checksum.as_ref(),
        )
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

fn alternate_line_endings(sql: &str) -> Option<String> {
    if sql.contains("\r\n") {
        Some(sql.replace("\r\n", "\n"))
    } else if sql.contains('\n') {
        Some(sql.replace('\n', "\r\n"))
    } else {
        None
    }
}

async fn update_checksum(
    transaction: &mut Transaction<'_, sqlx::Sqlite>,
    version: i64,
    stored_checksum: &[u8],
    current_checksum: &[u8],
) -> AppResult<()> {
    let result = sqlx::query(
        "UPDATE _sqlx_migrations SET checksum = ? WHERE version = ? AND checksum = ? AND success = 1",
    )
    .bind(current_checksum)
    .bind(version)
    .bind(stored_checksum)
    .execute(&mut **transaction)
    .await?;
    if result.rows_affected() != 1 {
        return Err(AppError::Integrity(format!(
            "migration {version} checksum changed while repairing line endings"
        )));
    }
    Ok(())
}
