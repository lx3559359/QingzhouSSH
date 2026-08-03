use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use uuid::Uuid;

use crate::{
    domain::execution::{
        now_millis, ExecutionDetails, ExecutionFile, ExecutionFilter, ExecutionParameter,
        ExecutionRecord, ExecutionStatus, FinishExecution, NewExecution,
    },
    error::{AppError, AppResult},
};

#[derive(Clone)]
pub struct ExecutionRepository {
    pool: SqlitePool,
}

impl ExecutionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, draft: NewExecution) -> AppResult<ExecutionRecord> {
        validate_draft(&draft)?;
        let id = Uuid::new_v4();
        let created_at = now_millis();
        let parameters_summary = parameter_summary(&draft.parameters);
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO executions (id,server_id,task_id,task_version,category,status,created_at,retryable,parameters_summary) VALUES (?,?,?,?,?,'queued',?,0,?)",
        )
        .bind(id.to_string())
        .bind(&draft.server_id)
        .bind(&draft.task_id)
        .bind(i64::from(draft.task_version))
        .bind(&draft.category)
        .bind(created_at)
        .bind(parameters_summary.as_deref())
        .execute(&mut *transaction)
        .await?;

        for parameter in &draft.parameters {
            let display_value = if parameter.sensitive {
                "[REDACTED]"
            } else {
                parameter.display_value.as_str()
            };
            sqlx::query(
                "INSERT INTO execution_parameters (execution_id,name,display_value,sensitive) VALUES (?,?,?,?)",
            )
            .bind(id.to_string())
            .bind(&parameter.name)
            .bind(display_value)
            .bind(parameter.sensitive)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        self.get(id)
            .await?
            .map(|details| details.record)
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn mark_running(&self, id: Uuid, started_at: i64) -> AppResult<()> {
        let result = sqlx::query(
            "UPDATE executions SET status='running',started_at=? WHERE id=? AND status='queued'",
        )
        .bind(started_at)
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        ensure_changed(result.rows_affected(), "执行记录无法进入运行状态")
    }

    pub async fn finish(&self, finish: FinishExecution) -> AppResult<()> {
        if !finish.status.is_terminal() {
            return Err(AppError::Validation("执行终态无效".into()));
        }
        let error_message = cap_utf8(finish.error_message, 8 * 1024);
        let output_summary = cap_utf8(finish.output_summary, 8 * 1024);
        let duration_ms = i64::try_from(finish.duration_ms)
            .map_err(|_| AppError::Validation("执行时长超出范围".into()))?;
        let result = sqlx::query(
            "UPDATE executions SET status=?,finished_at=?,duration_ms=?,exit_code=?,error_category=?,error_message=?,retryable=?,output_summary=?,remote_process_group=? WHERE id=? AND status IN ('queued','running')",
        )
        .bind(finish.status.as_str())
        .bind(finish.finished_at)
        .bind(duration_ms)
        .bind(finish.exit_code)
        .bind(finish.error_category)
        .bind(error_message)
        .bind(finish.retryable)
        .bind(output_summary)
        .bind(finish.remote_process_group)
        .bind(finish.id.to_string())
        .execute(&self.pool)
        .await?;
        ensure_changed(result.rows_affected(), "执行记录无法进入终态")
    }

    pub async fn add_file(&self, execution_id: Uuid, file: ExecutionFile) -> AppResult<()> {
        validate_file(&file)?;
        sqlx::query(
            "INSERT INTO execution_files (id,execution_id,relative_path,purpose,size_bytes,sha256) VALUES (?,?,?,?,?,?)",
        )
        .bind(file.id.to_string())
        .bind(execution_id.to_string())
        .bind(file.relative_path)
        .bind(file.purpose)
        .bind(i64::try_from(file.size_bytes).map_err(|_| {
            AppError::Validation("执行文件大小超出范围".into())
        })?)
        .bind(file.sha256.to_ascii_lowercase())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list(&self, filter: ExecutionFilter) -> AppResult<Vec<ExecutionRecord>> {
        let status = filter.status.map(|value| value.as_str().to_string());
        sqlx::query(
            "SELECT id,server_id,task_id,task_version,category,status,created_at,started_at,finished_at,duration_ms,exit_code,error_category,error_message,retryable,parameters_summary,output_summary,remote_process_group FROM executions WHERE (? IS NULL OR server_id=?) AND (? IS NULL OR category=?) AND (? IS NULL OR status=?) AND (? IS NULL OR created_at>=?) AND (? IS NULL OR created_at<=?) ORDER BY created_at DESC,id DESC LIMIT 500",
        )
            .bind(filter.server_id.as_deref())
            .bind(filter.server_id.as_deref())
            .bind(filter.category.as_deref())
            .bind(filter.category.as_deref())
            .bind(status.as_deref())
            .bind(status.as_deref())
            .bind(filter.created_from)
            .bind(filter.created_from)
            .bind(filter.created_to)
            .bind(filter.created_to)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(map_record)
            .collect()
    }

    pub async fn get(&self, id: Uuid) -> AppResult<Option<ExecutionDetails>> {
        let Some(row) = sqlx::query(
            "SELECT id,server_id,task_id,task_version,category,status,created_at,started_at,finished_at,duration_ms,exit_code,error_category,error_message,retryable,parameters_summary,output_summary,remote_process_group FROM executions WHERE id=?",
        )
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
        else {
            return Ok(None);
        };
        let record = map_record(&row)?;
        let parameters = sqlx::query(
            "SELECT name,display_value,sensitive FROM execution_parameters WHERE execution_id=? ORDER BY name",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(map_parameter)
        .collect::<AppResult<Vec<_>>>()?;
        let files = sqlx::query(
            "SELECT id,relative_path,purpose,size_bytes,sha256 FROM execution_files WHERE execution_id=? ORDER BY id",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(map_file)
        .collect::<AppResult<Vec<_>>>()?;
        Ok(Some(ExecutionDetails {
            record,
            parameters,
            files,
        }))
    }

    pub async fn recover_interrupted(&self) -> AppResult<u64> {
        let now = now_millis();
        let result = sqlx::query(
            "UPDATE executions SET status='uncertain',finished_at=?,duration_ms=CASE WHEN started_at IS NULL THEN 0 ELSE MAX(0,?-started_at) END,error_category='interrupted',error_message='应用退出时远程状态未确认',retryable=1 WHERE status='running'",
        )
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}

fn validate_draft(draft: &NewExecution) -> AppResult<()> {
    if draft.server_id.trim().is_empty()
        || draft.task_id.trim().is_empty()
        || draft.category.trim().is_empty()
        || draft.task_version <= 0
    {
        return Err(AppError::Validation("执行定义不完整".into()));
    }
    if !matches!(
        draft.category.as_str(),
        "system" | "service" | "logs" | "advanced" | "transfer"
    ) {
        return Err(AppError::Validation("执行类别无效".into()));
    }
    for parameter in &draft.parameters {
        if parameter.name.trim().is_empty() || parameter.name.contains('\0') {
            return Err(AppError::Validation("执行参数名称无效".into()));
        }
        if parameter.display_value.len() > 8 * 1024 {
            return Err(AppError::Validation("执行参数摘要超过限制".into()));
        }
    }
    Ok(())
}

fn validate_file(file: &ExecutionFile) -> AppResult<()> {
    if file.relative_path.is_empty()
        || file.relative_path.contains('\0')
        || std::path::Path::new(&file.relative_path).is_absolute()
        || file
            .relative_path
            .split(['/', '\\'])
            .any(|part| part == "..")
    {
        return Err(AppError::Validation(
            "执行文件必须使用安全的数据根相对路径".into(),
        ));
    }
    if file.sha256.len() != 64 || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::Validation("执行文件 SHA-256 无效".into()));
    }
    Ok(())
}

fn parameter_summary(parameters: &[ExecutionParameter]) -> Option<String> {
    if parameters.is_empty() {
        return None;
    }
    let summary = parameters
        .iter()
        .map(|parameter| {
            let value = if parameter.sensitive {
                "[REDACTED]"
            } else {
                &parameter.display_value
            };
            format!("{}={value}", parameter.name)
        })
        .collect::<Vec<_>>()
        .join(", ");
    cap_utf8(Some(summary), 8 * 1024)
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

fn ensure_changed(rows: u64, message: &str) -> AppResult<()> {
    if rows == 1 {
        Ok(())
    } else {
        Err(AppError::Validation(message.into()))
    }
}

fn map_record(row: &SqliteRow) -> AppResult<ExecutionRecord> {
    let id: String = row.try_get("id")?;
    let status: String = row.try_get("status")?;
    let duration_ms: Option<i64> = row.try_get("duration_ms")?;
    Ok(ExecutionRecord {
        id: Uuid::parse_str(&id)
            .map_err(|_| AppError::Validation(format!("数据库中的执行标识无效：{id}")))?,
        server_id: row.try_get("server_id")?,
        task_id: row.try_get("task_id")?,
        task_version: i32::try_from(row.try_get::<i64, _>("task_version")?)
            .map_err(|_| AppError::Validation("数据库中的任务版本无效".into()))?,
        category: row.try_get("category")?,
        status: ExecutionStatus::try_from(status.as_str())?,
        created_at: row.try_get("created_at")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        duration_ms: duration_ms
            .map(|value| {
                u64::try_from(value)
                    .map_err(|_| AppError::Validation("数据库中的执行时长无效".into()))
            })
            .transpose()?,
        exit_code: row.try_get("exit_code")?,
        error_category: row.try_get("error_category")?,
        error_message: row.try_get("error_message")?,
        retryable: row.try_get("retryable")?,
        parameters_summary: row.try_get("parameters_summary")?,
        output_summary: row.try_get("output_summary")?,
        remote_process_group: row.try_get("remote_process_group")?,
    })
}

fn map_parameter(row: &SqliteRow) -> AppResult<ExecutionParameter> {
    Ok(ExecutionParameter {
        name: row.try_get("name")?,
        display_value: row.try_get("display_value")?,
        sensitive: row.try_get("sensitive")?,
    })
}

fn map_file(row: &SqliteRow) -> AppResult<ExecutionFile> {
    let id: String = row.try_get("id")?;
    let size_bytes: i64 = row.try_get("size_bytes")?;
    Ok(ExecutionFile {
        id: Uuid::parse_str(&id)
            .map_err(|_| AppError::Validation(format!("数据库中的文件标识无效：{id}")))?,
        relative_path: row.try_get("relative_path")?,
        purpose: row.try_get("purpose")?,
        size_bytes: u64::try_from(size_bytes)
            .map_err(|_| AppError::Validation("数据库中的文件大小无效".into()))?,
        sha256: row.try_get("sha256")?,
    })
}
