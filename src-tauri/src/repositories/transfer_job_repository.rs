use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use uuid::Uuid;

use crate::{
    domain::{
        execution::now_millis,
        transfer_job::{NewTransferJob, TransferDirection, TransferJob, TransferJobStatus},
    },
    error::{AppError, AppResult},
};

#[derive(Debug, Clone)]
pub struct TransferJobFinish {
    pub status: TransferJobStatus,
    pub retryable: bool,
    pub error_category: Option<String>,
    pub error_message: Option<String>,
    pub sha256: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelTransferAction {
    CancelledQueued,
    SignalExecution(Uuid),
    AwaitExecutionId,
}

#[derive(Clone)]
pub struct TransferJobRepository {
    pool: SqlitePool,
}

impl TransferJobRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, draft: NewTransferJob) -> AppResult<TransferJob> {
        validate_draft(&draft)?;
        let id = Uuid::new_v4();
        let now = now_millis();
        sqlx::query(
            "INSERT INTO transfer_jobs (id,server_id,direction,source_path,target_path,overwrite,verification,status,created_at,updated_at) VALUES (?,?,?,?,?,?,?,'queued',?,?)",
        )
        .bind(id.to_string())
        .bind(draft.server_id)
        .bind(draft.direction.as_str())
        .bind(draft.source_path)
        .bind(draft.target_path)
        .bind(draft.overwrite)
        .bind(draft.verification)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.require(id).await
    }

    pub async fn get(&self, id: Uuid) -> AppResult<Option<TransferJob>> {
        sqlx::query("SELECT id,execution_id,server_id,direction,source_path,target_path,overwrite,verification,status,transferred,total,percent,bytes_per_second,average_bytes_per_second,eta_seconds,attempt_count,max_attempts,cancel_requested,retryable,error_category,error_message,sha256,location,created_at,updated_at,started_at,finished_at FROM transfer_jobs WHERE id=?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .map(map_job)
            .transpose()
    }

    pub async fn require(&self, id: Uuid) -> AppResult<TransferJob> {
        self.get(id)
            .await?
            .ok_or_else(|| AppError::Validation("传输任务不存在".into()))
    }

    pub async fn list(&self, server_id: Option<&str>) -> AppResult<Vec<TransferJob>> {
        sqlx::query("SELECT id,execution_id,server_id,direction,source_path,target_path,overwrite,verification,status,transferred,total,percent,bytes_per_second,average_bytes_per_second,eta_seconds,attempt_count,max_attempts,cancel_requested,retryable,error_category,error_message,sha256,location,created_at,updated_at,started_at,finished_at FROM transfer_jobs WHERE (? IS NULL OR server_id=?) ORDER BY created_at DESC,id DESC LIMIT 200")
            .bind(server_id)
            .bind(server_id)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(map_job)
            .collect()
    }

    pub async fn next_runnable(&self) -> AppResult<Option<TransferJob>> {
        sqlx::query("SELECT id,execution_id,server_id,direction,source_path,target_path,overwrite,verification,status,transferred,total,percent,bytes_per_second,average_bytes_per_second,eta_seconds,attempt_count,max_attempts,cancel_requested,retryable,error_category,error_message,sha256,location,created_at,updated_at,started_at,finished_at FROM transfer_jobs job WHERE status='queued' AND (SELECT COUNT(*) FROM transfer_jobs active WHERE active.server_id=job.server_id AND active.status IN ('connecting','transferring','verifying','finalizing')) < 2 ORDER BY created_at,id LIMIT 1")
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .map(map_job)
            .transpose()
    }

    pub async fn has_pending(&self) -> AppResult<bool> {
        Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM transfer_jobs WHERE status IN ('queued','connecting','transferring','verifying','finalizing'))",
        )
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn claim(&self, id: Uuid) -> AppResult<bool> {
        let now = now_millis();
        let result = sqlx::query(
            "UPDATE transfer_jobs SET status='connecting',attempt_count=attempt_count+1,cancel_requested=0,retryable=0,error_category=NULL,error_message=NULL,sha256=NULL,location=NULL,transferred=0,total=NULL,percent=NULL,bytes_per_second=NULL,average_bytes_per_second=NULL,eta_seconds=NULL,started_at=COALESCE(started_at,?),finished_at=NULL,updated_at=? WHERE id=? AND status='queued'",
        )
        .bind(now)
        .bind(now)
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn set_execution(&self, id: Uuid, execution_id: Uuid) -> AppResult<bool> {
        let result = sqlx::query(
            "UPDATE transfer_jobs SET execution_id=?,updated_at=? WHERE id=? AND status IN ('connecting','transferring','verifying','finalizing')",
        )
        .bind(execution_id.to_string())
        .bind(now_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_progress(
        &self,
        id: Uuid,
        status: TransferJobStatus,
        transferred: u64,
        total: Option<u64>,
        percent: Option<f64>,
        bytes_per_second: Option<f64>,
        average_bytes_per_second: Option<f64>,
        eta_seconds: Option<u64>,
    ) -> AppResult<()> {
        if !status.is_active() {
            return Err(AppError::Validation("传输进度状态无效".into()));
        }
        sqlx::query(
            "UPDATE transfer_jobs SET status=?,transferred=?,total=?,percent=?,bytes_per_second=?,average_bytes_per_second=?,eta_seconds=?,updated_at=? WHERE id=? AND status IN ('connecting','transferring','verifying','finalizing')",
        )
        .bind(status.as_str())
        .bind(to_i64(transferred, "已传输字节数")?)
        .bind(total.map(|value| to_i64(value, "文件总字节数")).transpose()?)
        .bind(percent)
        .bind(bytes_per_second)
        .bind(average_bytes_per_second)
        .bind(eta_seconds.map(|value| to_i64(value, "预计剩余秒数")).transpose()?)
        .bind(now_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn finish(&self, id: Uuid, finish: TransferJobFinish) -> AppResult<()> {
        if !finish.status.is_terminal() {
            return Err(AppError::Validation("传输任务终态无效".into()));
        }
        let now = now_millis();
        let result = sqlx::query(
            "UPDATE transfer_jobs SET status=?,retryable=?,error_category=?,error_message=?,sha256=?,location=?,percent=CASE WHEN ?='succeeded' THEN 100 ELSE percent END,finished_at=?,updated_at=? WHERE id=? AND status IN ('connecting','transferring','verifying','finalizing')",
        )
        .bind(finish.status.as_str())
        .bind(finish.retryable)
        .bind(finish.error_category)
        .bind(cap_utf8(finish.error_message, 8 * 1024))
        .bind(finish.sha256)
        .bind(finish.location)
        .bind(finish.status.as_str())
        .bind(now)
        .bind(now)
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 1 {
            Ok(())
        } else {
            Err(AppError::Validation("传输任务已经结束".into()))
        }
    }

    pub async fn request_cancel(&self, id: Uuid) -> AppResult<CancelTransferAction> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query("SELECT status,execution_id FROM transfer_jobs WHERE id=?")
            .bind(id.to_string())
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or_else(|| AppError::Validation("传输任务不存在".into()))?;
        let status: String = row.try_get("status")?;
        let status = TransferJobStatus::try_from(status.as_str())?;
        let execution_id: Option<String> = row.try_get("execution_id")?;
        let now = now_millis();
        let action = if status == TransferJobStatus::Queued {
            sqlx::query("UPDATE transfer_jobs SET status='cancelled',cancel_requested=1,retryable=0,finished_at=?,updated_at=? WHERE id=? AND status='queued'")
                .bind(now)
                .bind(now)
                .bind(id.to_string())
                .execute(&mut *transaction)
                .await?;
            CancelTransferAction::CancelledQueued
        } else if status.is_active() {
            sqlx::query("UPDATE transfer_jobs SET cancel_requested=1,updated_at=? WHERE id=?")
                .bind(now)
                .bind(id.to_string())
                .execute(&mut *transaction)
                .await?;
            match execution_id {
                Some(value) => CancelTransferAction::SignalExecution(parse_uuid(&value)?),
                None => CancelTransferAction::AwaitExecutionId,
            }
        } else {
            return Err(AppError::Validation("传输任务已经结束".into()));
        };
        transaction.commit().await?;
        Ok(action)
    }

    pub async fn should_cancel(&self, id: Uuid) -> AppResult<bool> {
        Ok(
            sqlx::query_scalar::<_, bool>("SELECT cancel_requested FROM transfer_jobs WHERE id=?")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await?
                .unwrap_or(false),
        )
    }

    pub async fn retry(&self, id: Uuid) -> AppResult<TransferJob> {
        let now = now_millis();
        let result = sqlx::query(
            "UPDATE transfer_jobs SET status='queued',execution_id=NULL,cancel_requested=0,retryable=0,error_category=NULL,error_message=NULL,finished_at=NULL,updated_at=? WHERE id=? AND status IN ('failed','uncertain') AND retryable=1 AND attempt_count < max_attempts",
        )
        .bind(now)
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(AppError::Validation(
                "该传输不能重试或已达到重试上限".into(),
            ));
        }
        self.require(id).await
    }

    pub async fn requeue_automatically(&self, id: Uuid) -> AppResult<bool> {
        let result = sqlx::query(
            "UPDATE transfer_jobs SET status='queued',execution_id=NULL,cancel_requested=0,error_category=NULL,error_message=NULL,finished_at=NULL,updated_at=? WHERE id=? AND status='failed' AND retryable=1 AND attempt_count < max_attempts",
        )
        .bind(now_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn recover_interrupted(&self) -> AppResult<u64> {
        let now = now_millis();
        let result = sqlx::query(
            "UPDATE transfer_jobs SET status='uncertain',retryable=1,error_category='interrupted',error_message='应用退出时传输结果未能确认',finished_at=?,updated_at=? WHERE status IN ('connecting','transferring','verifying','finalizing')",
        )
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}

fn validate_draft(draft: &NewTransferJob) -> AppResult<()> {
    if draft.server_id.trim().is_empty()
        || draft.source_path.trim().is_empty()
        || draft.target_path.trim().is_empty()
        || draft.source_path.contains('\0')
        || draft.target_path.contains('\0')
    {
        return Err(AppError::Validation("传输任务定义不完整".into()));
    }
    if !matches!(
        draft.verification.as_str(),
        "balanced" | "strict" | "transport_only"
    ) {
        return Err(AppError::Validation("传输校验策略无效".into()));
    }
    Ok(())
}

fn map_job(row: &SqliteRow) -> AppResult<TransferJob> {
    let id: String = row.try_get("id")?;
    let execution_id: Option<String> = row.try_get("execution_id")?;
    let direction: String = row.try_get("direction")?;
    let status: String = row.try_get("status")?;
    Ok(TransferJob {
        id: parse_uuid(&id)?,
        execution_id: execution_id.as_deref().map(parse_uuid).transpose()?,
        server_id: row.try_get("server_id")?,
        direction: TransferDirection::try_from(direction.as_str())?,
        source_path: row.try_get("source_path")?,
        target_path: row.try_get("target_path")?,
        overwrite: row.try_get("overwrite")?,
        verification: row.try_get("verification")?,
        status: TransferJobStatus::try_from(status.as_str())?,
        transferred: from_i64(row.try_get("transferred")?, "已传输字节数")?,
        total: row
            .try_get::<Option<i64>, _>("total")?
            .map(|value| from_i64(value, "文件总字节数"))
            .transpose()?,
        percent: row.try_get("percent")?,
        bytes_per_second: row.try_get("bytes_per_second")?,
        average_bytes_per_second: row.try_get("average_bytes_per_second")?,
        eta_seconds: row
            .try_get::<Option<i64>, _>("eta_seconds")?
            .map(|value| from_i64(value, "预计剩余秒数"))
            .transpose()?,
        attempt_count: from_u32(row.try_get("attempt_count")?, "重试次数")?,
        max_attempts: from_u32(row.try_get("max_attempts")?, "最大重试次数")?,
        cancel_requested: row.try_get("cancel_requested")?,
        retryable: row.try_get("retryable")?,
        error_category: row.try_get("error_category")?,
        error_message: row.try_get("error_message")?,
        sha256: row.try_get("sha256")?,
        location: row.try_get("location")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
    })
}

fn parse_uuid(value: &str) -> AppResult<Uuid> {
    Uuid::parse_str(value)
        .map_err(|_| AppError::Validation(format!("数据库中的传输标识无效：{value}")))
}

fn to_i64(value: u64, label: &str) -> AppResult<i64> {
    i64::try_from(value).map_err(|_| AppError::Validation(format!("{label}超出范围")))
}

fn from_i64(value: i64, label: &str) -> AppResult<u64> {
    u64::try_from(value).map_err(|_| AppError::Validation(format!("{label}无效")))
}

fn from_u32(value: i64, label: &str) -> AppResult<u32> {
    u32::try_from(value).map_err(|_| AppError::Validation(format!("{label}无效")))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            server::{AuthKind, ServerProfile},
            transfer_job::TransferDirection,
        },
        repositories::server_repository::ServerRepository,
    };

    async fn insert_server(pool: &SqlitePool, name: &str) -> ServerProfile {
        let credential_id = format!("credential-{name}");
        let server = ServerProfile::new(
            name,
            "127.0.0.1",
            22,
            "tester",
            AuthKind::Password,
            &credential_id,
        );
        ServerRepository::new(pool.clone())
            .insert(&server)
            .await
            .unwrap();
        server
    }

    fn draft(server_id: &str, source: &str) -> NewTransferJob {
        NewTransferJob {
            server_id: server_id.into(),
            direction: TransferDirection::Upload,
            source_path: source.into(),
            target_path: format!("/srv/{source}"),
            overwrite: false,
            verification: "balanced".into(),
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn persists_progress_and_cancels_queued_or_active_jobs(pool: SqlitePool) {
        let server = insert_server(&pool, "queue").await;
        let repository = TransferJobRepository::new(pool);
        let active = repository
            .create(draft(&server.id, "active.bin"))
            .await
            .unwrap();
        let queued = repository
            .create(draft(&server.id, "queued.bin"))
            .await
            .unwrap();

        assert!(repository.claim(active.id).await.unwrap());
        repository
            .update_progress(
                active.id,
                TransferJobStatus::Transferring,
                512,
                Some(1024),
                Some(50.0),
                Some(2048.0),
                Some(1024.0),
                Some(1),
            )
            .await
            .unwrap();
        assert_eq!(
            repository.request_cancel(active.id).await.unwrap(),
            CancelTransferAction::AwaitExecutionId
        );
        let active = repository.require(active.id).await.unwrap();
        assert_eq!(active.status, TransferJobStatus::Transferring);
        assert_eq!(active.transferred, 512);
        assert!(active.cancel_requested);

        assert_eq!(
            repository.request_cancel(queued.id).await.unwrap(),
            CancelTransferAction::CancelledQueued
        );
        assert_eq!(
            repository.require(queued.id).await.unwrap().status,
            TransferJobStatus::Cancelled
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn allows_two_jobs_per_server_before_scheduling_other_servers(pool: SqlitePool) {
        let first_server = insert_server(&pool, "first").await;
        let second_server = insert_server(&pool, "second").await;
        let repository = TransferJobRepository::new(pool);
        let first = repository
            .create(draft(&first_server.id, "first.bin"))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let same_server = repository
            .create(draft(&first_server.id, "same.bin"))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let third_same_server = repository
            .create(draft(&first_server.id, "third.bin"))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let other_server = repository
            .create(draft(&second_server.id, "other.bin"))
            .await
            .unwrap();

        assert!(repository.claim(first.id).await.unwrap());
        assert_eq!(
            repository.next_runnable().await.unwrap().unwrap().id,
            same_server.id
        );
        assert!(repository.claim(same_server.id).await.unwrap());
        assert_eq!(
            repository.next_runnable().await.unwrap().unwrap().id,
            other_server.id
        );
        assert_ne!(third_same_server.id, other_server.id);

        repository
            .finish(
                first.id,
                TransferJobFinish {
                    status: TransferJobStatus::Failed,
                    retryable: true,
                    error_category: Some("io".into()),
                    error_message: Some("connection reset".into()),
                    sha256: None,
                    location: None,
                },
            )
            .await
            .unwrap();
        let retried = repository.retry(first.id).await.unwrap();
        assert_eq!(retried.status, TransferJobStatus::Queued);
        assert_eq!(retried.attempt_count, 1);
    }
}
