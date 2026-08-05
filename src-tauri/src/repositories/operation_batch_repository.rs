use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use uuid::Uuid;

use crate::{
    domain::{
        execution::now_millis,
        operation_batch::{
            NewOperationBatch, OperationBatchDetails, OperationBatchItemRecord,
            OperationBatchItemStatus, OperationBatchRecord, OperationBatchStatus,
        },
    },
    error::{AppError, AppResult},
};

#[derive(Clone)]
pub struct OperationBatchRepository {
    pool: SqlitePool,
}

impl OperationBatchRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, draft: NewOperationBatch) -> AppResult<OperationBatchRecord> {
        if draft.task_id.trim().is_empty()
            || draft.task_id.contains('\0')
            || draft.task_version <= 0
            || draft.server_ids.is_empty()
        {
            return Err(AppError::Validation("批量任务定义不完整".into()));
        }
        let id = Uuid::new_v4();
        let now = now_millis();
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO operation_batches (id,task_id,task_version,status,created_at) VALUES (?,?,?,'queued',?)",
        )
        .bind(id.to_string())
        .bind(draft.task_id)
        .bind(i64::from(draft.task_version))
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        for server_id in draft.server_ids {
            sqlx::query(
                "INSERT INTO operation_batch_items (batch_id,server_id,status) VALUES (?,?,'queued')",
            )
            .bind(id.to_string())
            .bind(server_id)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        self.get(id)
            .await?
            .map(|details| details.batch)
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn mark_running(&self, id: Uuid) -> AppResult<()> {
        self.transition_batch(
            id,
            OperationBatchStatus::Queued,
            OperationBatchStatus::Running,
        )
        .await
    }

    pub async fn mark_item_running(&self, batch_id: Uuid, server_id: &str) -> AppResult<()> {
        let result = sqlx::query(
            "UPDATE operation_batch_items SET status='running' WHERE batch_id=? AND server_id=? AND status='queued'",
        )
        .bind(batch_id.to_string())
        .bind(server_id)
        .execute(&self.pool)
        .await?;
        ensure_one(result.rows_affected(), "批量子任务已被取消或启动")
    }

    pub async fn finish_item(
        &self,
        batch_id: Uuid,
        server_id: &str,
        status: OperationBatchItemStatus,
        operation_run_id: Option<Uuid>,
        error_message: Option<String>,
    ) -> AppResult<()> {
        if !status.is_terminal() {
            return Err(AppError::Validation("批量子任务终态无效".into()));
        }
        let result = sqlx::query(
            "UPDATE operation_batch_items SET status=?,operation_run_id=COALESCE(?,operation_run_id),error_message=? WHERE batch_id=? AND server_id=? AND status='running'",
        )
        .bind(status.as_str())
        .bind(operation_run_id.map(|id| id.to_string()))
        .bind(cap_utf8(error_message, 8 * 1024))
        .bind(batch_id.to_string())
        .bind(server_id)
        .execute(&self.pool)
        .await?;
        ensure_one(result.rows_affected(), "批量子任务状态已改变")
    }

    pub async fn cancel_queued_items(&self, id: Uuid) -> AppResult<u64> {
        let result = sqlx::query(
            "UPDATE operation_batch_items SET status='cancelled',error_message='用户取消，未开始执行' WHERE batch_id=? AND status='queued'",
        )
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn fail_nonterminal_items(&self, id: Uuid, message: &str) -> AppResult<u64> {
        let result = sqlx::query(
            "UPDATE operation_batch_items SET status='failed',error_message=? WHERE batch_id=? AND status IN ('queued','running')",
        )
        .bind(cap_utf8(Some(message.to_owned()), 8 * 1024))
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn complete(&self, id: Uuid, cancelled: bool) -> AppResult<OperationBatchStatus> {
        let rows = sqlx::query(
            "SELECT status,COUNT(*) AS count FROM operation_batch_items WHERE batch_id=? GROUP BY status",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mut succeeded = 0_i64;
        let mut failed = 0_i64;
        let mut cancelled_items = 0_i64;
        for row in rows {
            let status: String = row.try_get("status")?;
            let count: i64 = row.try_get("count")?;
            match OperationBatchItemStatus::try_from(status.as_str())? {
                OperationBatchItemStatus::Succeeded => succeeded += count,
                OperationBatchItemStatus::Failed => failed += count,
                OperationBatchItemStatus::Cancelled => cancelled_items += count,
                OperationBatchItemStatus::Queued | OperationBatchItemStatus::Running => {
                    return Err(AppError::Validation("批量任务仍有未结束的子任务".into()));
                }
            }
        }
        let next = if cancelled || cancelled_items > 0 {
            OperationBatchStatus::Cancelled
        } else if failed == 0 && succeeded > 0 {
            OperationBatchStatus::Succeeded
        } else if succeeded == 0 && failed > 0 {
            OperationBatchStatus::Failed
        } else {
            OperationBatchStatus::Partial
        };
        self.transition_batch(id, OperationBatchStatus::Running, next)
            .await?;
        Ok(next)
    }

    pub async fn get(&self, id: Uuid) -> AppResult<Option<OperationBatchDetails>> {
        let Some(row) = sqlx::query(
            "SELECT id,task_id,task_version,status,created_at,finished_at FROM operation_batches WHERE id=?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        let batch = map_batch(&row)?;
        let items = sqlx::query(
            "SELECT batch_id,server_id,operation_run_id,status,error_message FROM operation_batch_items WHERE batch_id=? ORDER BY rowid",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(map_item)
        .collect::<AppResult<Vec<_>>>()?;
        Ok(Some(OperationBatchDetails { batch, items }))
    }

    pub async fn recover_interrupted(&self) -> AppResult<u64> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "UPDATE operation_batch_items SET status='failed',error_message='应用上次退出，子任务状态未确认' WHERE status IN ('queued','running') AND batch_id IN (SELECT id FROM operation_batches WHERE status IN ('queued','running'))",
        )
        .execute(&mut *transaction)
        .await?;
        let result = sqlx::query(
            "UPDATE operation_batches SET status='failed',finished_at=? WHERE status IN ('queued','running')",
        )
        .bind(now_millis())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(result.rows_affected())
    }

    async fn transition_batch(
        &self,
        id: Uuid,
        current: OperationBatchStatus,
        next: OperationBatchStatus,
    ) -> AppResult<()> {
        let result = sqlx::query(
            "UPDATE operation_batches SET status=?,finished_at=? WHERE id=? AND status=?",
        )
        .bind(next.as_str())
        .bind(next.is_terminal().then(now_millis))
        .bind(id.to_string())
        .bind(current.as_str())
        .execute(&self.pool)
        .await?;
        ensure_one(result.rows_affected(), "批量任务状态已改变")
    }
}

fn map_batch(row: &SqliteRow) -> AppResult<OperationBatchRecord> {
    let id: String = row.try_get("id")?;
    let status: String = row.try_get("status")?;
    Ok(OperationBatchRecord {
        id: parse_uuid(&id, "批量任务")?,
        task_id: row.try_get("task_id")?,
        task_version: i32::try_from(row.try_get::<i64, _>("task_version")?)
            .map_err(|_| AppError::Validation("数据库中的批量任务版本无效".into()))?,
        status: OperationBatchStatus::try_from(status.as_str())?,
        created_at: row.try_get("created_at")?,
        finished_at: row.try_get("finished_at")?,
    })
}

fn map_item(row: &SqliteRow) -> AppResult<OperationBatchItemRecord> {
    let batch_id: String = row.try_get("batch_id")?;
    let operation_run_id: Option<String> = row.try_get("operation_run_id")?;
    let status: String = row.try_get("status")?;
    Ok(OperationBatchItemRecord {
        batch_id: parse_uuid(&batch_id, "批量任务")?,
        server_id: row.try_get("server_id")?,
        operation_run_id: operation_run_id
            .map(|id| parse_uuid(&id, "运维运行"))
            .transpose()?,
        status: OperationBatchItemStatus::try_from(status.as_str())?,
        error_message: row.try_get("error_message")?,
    })
}

fn parse_uuid(value: &str, kind: &str) -> AppResult<Uuid> {
    Uuid::parse_str(value)
        .map_err(|_| AppError::Validation(format!("数据库中的{kind}标识无效：{value}")))
}

fn ensure_one(rows: u64, message: &str) -> AppResult<()> {
    if rows == 1 {
        Ok(())
    } else {
        Err(AppError::Validation(message.into()))
    }
}

fn cap_utf8(value: Option<String>, max_bytes: usize) -> Option<String> {
    value.map(|mut value| {
        if value.len() > max_bytes {
            let mut boundary = max_bytes;
            while !value.is_char_boundary(boundary) {
                boundary -= 1;
            }
            value.truncate(boundary);
        }
        value
    })
}
