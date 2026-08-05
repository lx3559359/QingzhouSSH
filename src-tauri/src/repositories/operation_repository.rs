use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use uuid::Uuid;

use crate::{
    core::tasks::RiskLevel,
    domain::{
        execution::now_millis,
        operation::{
            FinishOperationStep, NewOperationRun, NewOperationStep, OperationDetails,
            OperationPhase, OperationRunRecord, OperationStatus, OperationStepRecord,
            OperationStepStatus,
        },
    },
    error::{AppError, AppResult},
};

#[derive(Clone)]
pub struct OperationRepository {
    pool: SqlitePool,
}

impl OperationRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, draft: NewOperationRun) -> AppResult<OperationRunRecord> {
        validate_run(&draft)?;
        let id = Uuid::new_v4();
        let now = now_millis();
        sqlx::query(
            "INSERT INTO operation_runs (id,server_id,task_id,task_version,risk_level,status,parameters_summary,created_at,updated_at) VALUES (?,?,?,?,?,'validating',?,?,?)",
        )
        .bind(id.to_string())
        .bind(draft.server_id)
        .bind(draft.task_id)
        .bind(i64::from(draft.task_version))
        .bind(draft.risk_level.as_str())
        .bind(draft.parameters_summary)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.get(id)
            .await?
            .map(|details| details.run)
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn transition(&self, id: Uuid, next: OperationStatus) -> AppResult<()> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query("SELECT status FROM operation_runs WHERE id=?")
            .bind(id.to_string())
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or_else(|| AppError::Validation("运维运行不存在".into()))?;
        let current: String = row.try_get("status")?;
        let current = OperationStatus::try_from(current.as_str())?;
        if !current.can_transition_to(next) {
            return Err(AppError::Validation(format!(
                "运维状态不能从 {} 变为 {}",
                current.as_str(),
                next.as_str()
            )));
        }
        let now = now_millis();
        let finished_at = next.is_terminal().then_some(now);
        let result = sqlx::query(
            "UPDATE operation_runs SET status=?,updated_at=?,finished_at=? WHERE id=? AND status=?",
        )
        .bind(next.as_str())
        .bind(now)
        .bind(finished_at)
        .bind(id.to_string())
        .bind(current.as_str())
        .execute(&mut *transaction)
        .await?;
        ensure_changed(result.rows_affected(), "运维状态已被其他操作修改")?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn create_step(&self, draft: NewOperationStep) -> AppResult<OperationStepRecord> {
        validate_step(&draft)?;
        let step_index = index_to_i64(draft.step_index)?;
        sqlx::query(
            "INSERT INTO operation_steps (run_id,phase,step_index,step_id,title,status) VALUES (?,?,?,?,?,'pending')",
        )
        .bind(draft.run_id.to_string())
        .bind(draft.phase.as_str())
        .bind(step_index)
        .bind(draft.step_id)
        .bind(draft.title)
        .execute(&self.pool)
        .await?;
        self.get_step(draft.run_id, draft.phase, draft.step_index)
            .await?
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn mark_step_running(
        &self,
        run_id: Uuid,
        phase: OperationPhase,
        step_index: usize,
        started_at: i64,
    ) -> AppResult<()> {
        self.transition_step(
            run_id,
            phase,
            step_index,
            OperationStepStatus::Running,
            None,
            None,
            None,
            Some(started_at),
            None,
        )
        .await
    }

    pub async fn finish_step(&self, finish: FinishOperationStep) -> AppResult<()> {
        if !finish.status.is_terminal() {
            return Err(AppError::Validation("运维步骤终态无效".into()));
        }
        self.transition_step(
            finish.run_id,
            finish.phase,
            finish.step_index,
            finish.status,
            finish.execution_id,
            cap_utf8(finish.output_summary, 8 * 1024),
            cap_utf8(finish.error_message, 8 * 1024),
            None,
            Some(finish.finished_at),
        )
        .await
    }

    pub async fn get(&self, id: Uuid) -> AppResult<Option<OperationDetails>> {
        let Some(row) = sqlx::query(
            "SELECT id,server_id,task_id,task_version,risk_level,status,parameters_summary,result_json,error_category,error_message,created_at,updated_at,finished_at FROM operation_runs WHERE id=?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        let run = map_run(&row)?;
        let steps = sqlx::query(
            "SELECT run_id,phase,step_index,step_id,title,status,execution_id,output_summary,error_message,started_at,finished_at FROM operation_steps WHERE run_id=? ORDER BY CASE phase WHEN 'preflight' THEN 0 WHEN 'backup' THEN 1 WHEN 'execute' THEN 2 WHEN 'verify' THEN 3 ELSE 4 END,step_index",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(map_step)
        .collect::<AppResult<Vec<_>>>()?;
        Ok(Some(OperationDetails { run, steps }))
    }

    pub async fn recover_interrupted(&self) -> AppResult<u64> {
        let now = now_millis();
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "UPDATE operation_steps SET status='uncertain',finished_at=?,error_message='应用退出时步骤状态未确认' WHERE status='running' AND run_id IN (SELECT id FROM operation_runs WHERE status IN ('preflighting','backing_up','running','verifying','rolling_back'))",
        )
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        let result = sqlx::query(
            "UPDATE operation_runs SET status='uncertain',updated_at=?,finished_at=?,error_category='interrupted',error_message='应用退出时远程状态未确认' WHERE status IN ('preflighting','backing_up','running','verifying','rolling_back')",
        )
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(result.rows_affected())
    }

    #[allow(clippy::too_many_arguments)]
    async fn transition_step(
        &self,
        run_id: Uuid,
        phase: OperationPhase,
        step_index: usize,
        next: OperationStepStatus,
        execution_id: Option<Uuid>,
        output_summary: Option<String>,
        error_message: Option<String>,
        started_at: Option<i64>,
        finished_at: Option<i64>,
    ) -> AppResult<()> {
        let mut transaction = self.pool.begin().await?;
        let index = index_to_i64(step_index)?;
        let row = sqlx::query(
            "SELECT status FROM operation_steps WHERE run_id=? AND phase=? AND step_index=?",
        )
        .bind(run_id.to_string())
        .bind(phase.as_str())
        .bind(index)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| AppError::Validation("运维步骤不存在".into()))?;
        let current: String = row.try_get("status")?;
        let current = OperationStepStatus::try_from(current.as_str())?;
        if !current.can_transition_to(next) {
            return Err(AppError::Validation(format!(
                "运维步骤不能从 {} 变为 {}",
                current.as_str(),
                next.as_str()
            )));
        }
        let result = sqlx::query(
            "UPDATE operation_steps SET status=?,execution_id=COALESCE(?,execution_id),output_summary=COALESCE(?,output_summary),error_message=COALESCE(?,error_message),started_at=COALESCE(?,started_at),finished_at=COALESCE(?,finished_at) WHERE run_id=? AND phase=? AND step_index=? AND status=?",
        )
        .bind(next.as_str())
        .bind(execution_id.map(|id| id.to_string()))
        .bind(output_summary)
        .bind(error_message)
        .bind(started_at)
        .bind(finished_at)
        .bind(run_id.to_string())
        .bind(phase.as_str())
        .bind(index)
        .bind(current.as_str())
        .execute(&mut *transaction)
        .await?;
        ensure_changed(result.rows_affected(), "运维步骤已被其他操作修改")?;
        transaction.commit().await?;
        Ok(())
    }

    async fn get_step(
        &self,
        run_id: Uuid,
        phase: OperationPhase,
        step_index: usize,
    ) -> AppResult<Option<OperationStepRecord>> {
        sqlx::query(
            "SELECT run_id,phase,step_index,step_id,title,status,execution_id,output_summary,error_message,started_at,finished_at FROM operation_steps WHERE run_id=? AND phase=? AND step_index=?",
        )
        .bind(run_id.to_string())
        .bind(phase.as_str())
        .bind(index_to_i64(step_index)?)
        .fetch_optional(&self.pool)
        .await?
        .map(|row| map_step(&row))
        .transpose()
    }
}

fn validate_run(draft: &NewOperationRun) -> AppResult<()> {
    if draft.server_id.trim().is_empty()
        || draft.task_id.trim().is_empty()
        || draft.task_version <= 0
        || draft.server_id.contains('\0')
        || draft.task_id.contains('\0')
    {
        return Err(AppError::Validation("运维运行定义不完整".into()));
    }
    if draft
        .parameters_summary
        .as_ref()
        .is_some_and(|value| value.len() > 8 * 1024 || value.contains('\0'))
    {
        return Err(AppError::Validation("运维参数摘要无效".into()));
    }
    Ok(())
}

fn validate_step(draft: &NewOperationStep) -> AppResult<()> {
    if draft.step_id.trim().is_empty()
        || draft.title.trim().is_empty()
        || draft.step_id.contains('\0')
        || draft.title.contains('\0')
        || draft.step_id.len() > 200
        || draft.title.len() > 500
    {
        return Err(AppError::Validation("运维步骤定义无效".into()));
    }
    index_to_i64(draft.step_index)?;
    Ok(())
}

fn map_run(row: &SqliteRow) -> AppResult<OperationRunRecord> {
    let id: String = row.try_get("id")?;
    let risk: String = row.try_get("risk_level")?;
    let status: String = row.try_get("status")?;
    let result_json: Option<String> = row.try_get("result_json")?;
    Ok(OperationRunRecord {
        id: parse_uuid(&id, "运维运行")?,
        server_id: row.try_get("server_id")?,
        task_id: row.try_get("task_id")?,
        task_version: i32::try_from(row.try_get::<i64, _>("task_version")?)
            .map_err(|_| AppError::Validation("数据库中的运维任务版本无效".into()))?,
        risk_level: parse_risk(&risk)?,
        status: OperationStatus::try_from(status.as_str())?,
        parameters_summary: row.try_get("parameters_summary")?,
        result: result_json
            .map(|value| {
                serde_json::from_str(&value)
                    .map_err(|_| AppError::Validation("数据库中的运维结果无效".into()))
            })
            .transpose()?,
        error_category: row.try_get("error_category")?,
        error_message: row.try_get("error_message")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        finished_at: row.try_get("finished_at")?,
    })
}

fn map_step(row: &SqliteRow) -> AppResult<OperationStepRecord> {
    let run_id: String = row.try_get("run_id")?;
    let phase: String = row.try_get("phase")?;
    let status: String = row.try_get("status")?;
    let execution_id: Option<String> = row.try_get("execution_id")?;
    let step_index: i64 = row.try_get("step_index")?;
    Ok(OperationStepRecord {
        run_id: parse_uuid(&run_id, "运维运行")?,
        phase: OperationPhase::try_from(phase.as_str())?,
        step_index: usize::try_from(step_index)
            .map_err(|_| AppError::Validation("数据库中的运维步骤序号无效".into()))?,
        step_id: row.try_get("step_id")?,
        title: row.try_get("title")?,
        status: OperationStepStatus::try_from(status.as_str())?,
        execution_id: execution_id
            .map(|value| parse_uuid(&value, "执行记录"))
            .transpose()?,
        output_summary: row.try_get("output_summary")?,
        error_message: row.try_get("error_message")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
    })
}

fn parse_risk(value: &str) -> AppResult<RiskLevel> {
    match value {
        "safe" => Ok(RiskLevel::Safe),
        "caution" => Ok(RiskLevel::Caution),
        "dangerous" => Ok(RiskLevel::Dangerous),
        other => Err(AppError::Validation(format!("未知运维风险等级：{other}"))),
    }
}

fn parse_uuid(value: &str, kind: &str) -> AppResult<Uuid> {
    Uuid::parse_str(value)
        .map_err(|_| AppError::Validation(format!("数据库中的{kind}标识无效：{value}")))
}

fn index_to_i64(value: usize) -> AppResult<i64> {
    i64::try_from(value).map_err(|_| AppError::Validation("运维步骤序号超出范围".into()))
}

fn ensure_changed(rows: u64, message: &str) -> AppResult<()> {
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
