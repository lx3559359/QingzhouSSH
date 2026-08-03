use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use uuid::Uuid;

use crate::{
    core::{sftp::validate_remote_path, workflows::validate_restore_point_relative_path},
    domain::{
        execution::now_millis,
        workflow::{
            FinishWorkflowNode, FinishWorkflowRestorePoint, NewWorkflowRestorePoint,
            NewWorkflowRun, WorkflowDefinition, WorkflowDraft, WorkflowEdge, WorkflowNode,
            WorkflowNodeRun, WorkflowNodeStatus, WorkflowRestorePoint, WorkflowRestorePointStatus,
            WorkflowRunDetails, WorkflowRunEvent, WorkflowRunFilter, WorkflowRunRecord,
            WorkflowRunStatus, WorkflowSummary,
        },
    },
    error::{AppError, AppResult},
};

#[derive(Clone)]
pub struct WorkflowRepository {
    pool: SqlitePool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredWorkflowVersionRef<'a> {
    name: &'a str,
    description: &'a str,
    nodes: &'a [WorkflowNode],
    edges: &'a [WorkflowEdge],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredWorkflowVersion {
    name: String,
    description: String,
    nodes: Vec<WorkflowNode>,
    edges: Vec<WorkflowEdge>,
}

impl WorkflowRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn save(&self, draft: WorkflowDraft) -> AppResult<WorkflowDefinition> {
        validate_draft(&draft)?;
        let id = draft.id.unwrap_or_else(Uuid::new_v4);
        let stored = StoredWorkflowVersionRef {
            name: &draft.name,
            description: &draft.description,
            nodes: &draft.nodes,
            edges: &draft.edges,
        };
        let definition_json = serde_json::to_string(&stored)
            .map_err(|_| AppError::Validation("工作流定义无法序列化".into()))?;
        let checksum_sha256 = hex_sha256(definition_json.as_bytes());
        let now = now_millis();
        let mut transaction = self.pool.begin().await?;

        let existing = sqlx::query(
            "SELECT w.current_version,v.checksum_sha256 FROM workflows w JOIN workflow_versions v ON v.workflow_id=w.id AND v.version=w.current_version WHERE w.id=?",
        )
        .bind(id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;

        let version = if let Some(row) = existing {
            let current_version: i64 = row.try_get("current_version")?;
            let existing_checksum: String = row.try_get("checksum_sha256")?;
            if existing_checksum == checksum_sha256 {
                i32::try_from(current_version)
                    .map_err(|_| AppError::Validation("工作流版本超出范围".into()))?
            } else {
                let next = current_version
                    .checked_add(1)
                    .ok_or_else(|| AppError::Validation("工作流版本超出范围".into()))?;
                sqlx::query(
                    "INSERT INTO workflow_versions (workflow_id,version,definition_json,checksum_sha256,created_at) VALUES (?,?,?,?,?)",
                )
                .bind(id.to_string())
                .bind(next)
                .bind(&definition_json)
                .bind(&checksum_sha256)
                .bind(now)
                .execute(&mut *transaction)
                .await?;
                sqlx::query(
                    "UPDATE workflows SET name=?,description=?,current_version=?,updated_at=? WHERE id=?",
                )
                .bind(&draft.name)
                .bind(&draft.description)
                .bind(next)
                .bind(now)
                .bind(id.to_string())
                .execute(&mut *transaction)
                .await?;
                i32::try_from(next)
                    .map_err(|_| AppError::Validation("工作流版本超出范围".into()))?
            }
        } else {
            sqlx::query(
                "INSERT INTO workflows (id,name,description,current_version,created_at,updated_at) VALUES (?,?,?,1,?,?)",
            )
            .bind(id.to_string())
            .bind(&draft.name)
            .bind(&draft.description)
            .bind(now)
            .bind(now)
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "INSERT INTO workflow_versions (workflow_id,version,definition_json,checksum_sha256,created_at) VALUES (?,1,?,?,?)",
            )
            .bind(id.to_string())
            .bind(&definition_json)
            .bind(&checksum_sha256)
            .bind(now)
            .execute(&mut *transaction)
            .await?;
            1
        };

        transaction.commit().await?;
        Ok(WorkflowDefinition {
            id,
            name: draft.name,
            description: draft.description,
            version,
            checksum_sha256,
            nodes: draft.nodes,
            edges: draft.edges,
        })
    }

    pub async fn list(&self) -> AppResult<Vec<WorkflowSummary>> {
        sqlx::query(
            "SELECT id,name,description,current_version,created_at,updated_at FROM workflows ORDER BY updated_at DESC,id DESC",
        )
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(map_summary)
        .collect()
    }

    pub async fn get(
        &self,
        id: Uuid,
        version: Option<i32>,
    ) -> AppResult<Option<WorkflowDefinition>> {
        let row = sqlx::query(
            "SELECT w.id,v.version,v.definition_json,v.checksum_sha256 FROM workflows w JOIN workflow_versions v ON v.workflow_id=w.id WHERE w.id=? AND v.version=COALESCE(?,w.current_version)",
        )
        .bind(id.to_string())
        .bind(version.map(i64::from))
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| map_definition(&row)).transpose()
    }

    pub async fn create_run(&self, draft: NewWorkflowRun) -> AppResult<WorkflowRunRecord> {
        if draft.server_id.trim().is_empty() || draft.workflow_version <= 0 {
            return Err(AppError::Validation("工作流运行参数不完整".into()));
        }
        let id = Uuid::new_v4();
        let created_at = now_millis();
        sqlx::query(
            "INSERT INTO workflow_runs (id,workflow_id,workflow_version,server_id,status,created_at,retryable) VALUES (?,?,?,?,'queued',?,0)",
        )
        .bind(id.to_string())
        .bind(draft.workflow_id.to_string())
        .bind(i64::from(draft.workflow_version))
        .bind(draft.server_id)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        self.get_run_record(id)
            .await?
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn mark_run_running(
        &self,
        run_id: Uuid,
        current_node_id: Uuid,
        started_at: i64,
    ) -> AppResult<()> {
        let result = sqlx::query(
            "UPDATE workflow_runs SET status='running',current_node_id=?,started_at=? WHERE id=? AND status='queued'",
        )
        .bind(current_node_id.to_string())
        .bind(started_at)
        .bind(run_id.to_string())
        .execute(&self.pool)
        .await?;
        ensure_one(result.rows_affected(), "工作流无法进入运行状态")
    }

    pub async fn start_node_attempt(
        &self,
        run_id: Uuid,
        node_id: Uuid,
        started_at: i64,
    ) -> AppResult<WorkflowNodeRun> {
        let mut transaction = self.pool.begin().await?;
        let next_attempt: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(attempt),0)+1 FROM workflow_node_runs WHERE run_id=? AND node_id=?",
        )
        .bind(run_id.to_string())
        .bind(node_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO workflow_node_runs (run_id,node_id,attempt,status,started_at,retryable) VALUES (?,?,?,'running',?,0)",
        )
        .bind(run_id.to_string())
        .bind(node_id.to_string())
        .bind(next_attempt)
        .bind(started_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        self.get_node_attempt(
            run_id,
            node_id,
            i32::try_from(next_attempt)
                .map_err(|_| AppError::Validation("节点尝试次数超出范围".into()))?,
        )
        .await?
        .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn link_node_execution(
        &self,
        run_id: Uuid,
        node_id: Uuid,
        attempt: i32,
        execution_id: Uuid,
    ) -> AppResult<()> {
        let result = sqlx::query(
            "UPDATE workflow_node_runs SET execution_id=? WHERE run_id=? AND node_id=? AND attempt=? AND status='running'",
        )
        .bind(execution_id.to_string())
        .bind(run_id.to_string())
        .bind(node_id.to_string())
        .bind(i64::from(attempt))
        .execute(&self.pool)
        .await?;
        ensure_one(result.rows_affected(), "节点执行引用无法保存")
    }

    pub async fn finish_node(&self, finish: FinishWorkflowNode) -> AppResult<()> {
        if !finish.status.is_terminal() {
            return Err(AppError::Validation("工作流节点终态无效".into()));
        }
        let result = sqlx::query(
            "UPDATE workflow_node_runs SET status=?,finished_at=?,duration_ms=CASE WHEN started_at IS NULL THEN 0 ELSE MAX(0,?-started_at) END,exit_code=?,output_summary=?,error_message=?,retryable=? WHERE run_id=? AND node_id=? AND attempt=? AND status='running'",
        )
        .bind(finish.status.as_str())
        .bind(finish.finished_at)
        .bind(finish.finished_at)
        .bind(finish.exit_code)
        .bind(cap_utf8(finish.output_summary, 8 * 1024))
        .bind(cap_utf8(finish.error_message, 8 * 1024))
        .bind(finish.retryable)
        .bind(finish.run_id.to_string())
        .bind(finish.node_id.to_string())
        .bind(i64::from(finish.attempt))
        .execute(&self.pool)
        .await?;
        ensure_one(result.rows_affected(), "工作流节点无法进入终态")
    }

    pub async fn create_restore_point(
        &self,
        draft: NewWorkflowRestorePoint,
    ) -> AppResult<WorkflowRestorePoint> {
        validate_remote_path(&draft.remote_path)?;
        if let Some(relative_path) = &draft.relative_path {
            validate_restore_point_relative_path(relative_path)?;
        }
        let applicability_json = serde_json::to_string(&draft.applicability)
            .map_err(|_| AppError::Validation("恢复点适用条件无法序列化".into()))?;
        if applicability_json.len() > 8 * 1024 {
            return Err(AppError::Validation("恢复点适用条件超过 8 KiB".into()));
        }
        let id = Uuid::new_v4();
        let now = now_millis();
        sqlx::query(
            "INSERT INTO workflow_restore_points (id,run_id,node_id,remote_path,relative_path,original_existed,size_bytes,sha256,status,applicability_json,error_message,created_at,updated_at) VALUES (?,?,?,?,?,0,NULL,NULL,'creating',?,NULL,?,?)",
        )
        .bind(id.to_string())
        .bind(draft.run_id.to_string())
        .bind(draft.node_id.to_string())
        .bind(draft.remote_path)
        .bind(draft.relative_path)
        .bind(applicability_json)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.get_restore_point(id)
            .await?
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn finish_restore_point(
        &self,
        mut finish: FinishWorkflowRestorePoint,
    ) -> AppResult<()> {
        if !matches!(
            finish.status,
            WorkflowRestorePointStatus::Available | WorkflowRestorePointStatus::Failed
        ) {
            return Err(AppError::Validation("恢复点创建结果状态无效".into()));
        }
        if let Some(relative_path) = &finish.relative_path {
            validate_restore_point_relative_path(relative_path)?;
        }
        match finish.status {
            WorkflowRestorePointStatus::Available if finish.original_existed => {
                let valid_hash = finish.sha256.as_deref().is_some_and(|hash| {
                    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                });
                if finish.relative_path.is_none() || finish.size_bytes.is_none() || !valid_hash {
                    return Err(AppError::Validation(
                        "已有远程文件的恢复点缺少备份路径、大小或 SHA-256".into(),
                    ));
                }
                finish.error_message = None;
            }
            WorkflowRestorePointStatus::Available => {
                finish.relative_path = None;
                finish.size_bytes = None;
                finish.sha256 = None;
                finish.error_message = None;
            }
            WorkflowRestorePointStatus::Failed => {
                finish.original_existed = false;
                finish.relative_path = None;
                finish.size_bytes = None;
                finish.sha256 = None;
                if finish.error_message.as_deref().is_none_or(str::is_empty) {
                    finish.error_message = Some("恢复点创建失败".into());
                }
            }
            _ => unreachable!(),
        }
        let size_bytes = finish
            .size_bytes
            .map(i64::try_from)
            .transpose()
            .map_err(|_| AppError::Validation("恢复点大小超出范围".into()))?;
        let result = sqlx::query(
            "UPDATE workflow_restore_points SET status=?,original_existed=?,relative_path=?,size_bytes=?,sha256=?,error_message=?,updated_at=? WHERE id=? AND status='creating'",
        )
        .bind(finish.status.as_str())
        .bind(finish.original_existed)
        .bind(finish.relative_path)
        .bind(size_bytes)
        .bind(finish.sha256)
        .bind(cap_utf8(finish.error_message, 8 * 1024))
        .bind(now_millis())
        .bind(finish.id.to_string())
        .execute(&self.pool)
        .await?;
        ensure_one(result.rows_affected(), "恢复点不处于可完成状态")
    }

    pub async fn get_restore_point(&self, id: Uuid) -> AppResult<Option<WorkflowRestorePoint>> {
        sqlx::query(
            "SELECT id,run_id,node_id,remote_path,relative_path,original_existed,size_bytes,sha256,status,applicability_json,error_message,created_at,updated_at FROM workflow_restore_points WHERE id=?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .map(|row| map_restore_point(&row))
        .transpose()
    }

    pub async fn list_restore_points(&self, run_id: Uuid) -> AppResult<Vec<WorkflowRestorePoint>> {
        sqlx::query(
            "SELECT id,run_id,node_id,remote_path,relative_path,original_existed,size_bytes,sha256,status,applicability_json,error_message,created_at,updated_at FROM workflow_restore_points WHERE run_id=? ORDER BY created_at,id",
        )
        .bind(run_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(map_restore_point)
        .collect()
    }

    pub async fn append_event(
        &self,
        run_id: Uuid,
        event_type: &str,
        payload: Value,
        emitted_at: i64,
    ) -> AppResult<WorkflowRunEvent> {
        if event_type.trim().is_empty() || event_type.len() > 100 {
            return Err(AppError::Validation("工作流事件类型无效".into()));
        }
        let payload_json = serde_json::to_string(&payload)
            .map_err(|_| AppError::Validation("工作流事件无法序列化".into()))?;
        if payload_json.len() > 32 * 1024 {
            return Err(AppError::Validation("工作流事件超过 32 KiB".into()));
        }
        let sequence: i64 = sqlx::query_scalar(
            "INSERT INTO workflow_run_events (run_id,sequence,event_type,payload_json,emitted_at) SELECT ?,COALESCE(MAX(sequence),0)+1,?,?,? FROM workflow_run_events WHERE run_id=? RETURNING sequence",
        )
        .bind(run_id.to_string())
        .bind(event_type)
        .bind(&payload_json)
        .bind(emitted_at)
        .bind(run_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        Ok(WorkflowRunEvent {
            run_id,
            sequence,
            event_type: event_type.into(),
            payload,
            emitted_at,
        })
    }

    pub async fn get_run(&self, run_id: Uuid) -> AppResult<Option<WorkflowRunDetails>> {
        let Some(run) = self.get_run_record(run_id).await? else {
            return Ok(None);
        };
        let node_runs = sqlx::query(
            "SELECT run_id,node_id,attempt,status,execution_id,started_at,finished_at,duration_ms,exit_code,output_summary,error_message,retryable FROM workflow_node_runs WHERE run_id=? ORDER BY started_at,node_id,attempt",
        )
        .bind(run_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(map_node_run)
        .collect::<AppResult<Vec<_>>>()?;
        let events = sqlx::query(
            "SELECT run_id,sequence,event_type,payload_json,emitted_at FROM workflow_run_events WHERE run_id=? ORDER BY sequence",
        )
        .bind(run_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(map_event)
        .collect::<AppResult<Vec<_>>>()?;
        let restore_points = self.list_restore_points(run_id).await?;
        Ok(Some(WorkflowRunDetails {
            run,
            node_runs,
            restore_points,
            events,
        }))
    }

    pub async fn list_runs(&self, filter: WorkflowRunFilter) -> AppResult<Vec<WorkflowRunRecord>> {
        let status = filter.status.map(|status| status.as_str().to_string());
        let workflow_id = filter.workflow_id.map(|id| id.to_string());
        sqlx::query(
            "SELECT id,workflow_id,workflow_version,server_id,status,current_node_id,created_at,started_at,finished_at,duration_ms,error_category,error_message,retryable FROM workflow_runs WHERE (? IS NULL OR workflow_id=?) AND (? IS NULL OR server_id=?) AND (? IS NULL OR status=?) AND (? IS NULL OR created_at>=?) AND (? IS NULL OR created_at<=?) ORDER BY created_at DESC,id DESC LIMIT 500",
        )
        .bind(workflow_id.as_deref())
        .bind(workflow_id.as_deref())
        .bind(filter.server_id.as_deref())
        .bind(filter.server_id.as_deref())
        .bind(status.as_deref())
        .bind(status.as_deref())
        .bind(filter.created_from)
        .bind(filter.created_from)
        .bind(filter.created_to)
        .bind(filter.created_to)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(map_run)
        .collect()
    }

    pub async fn recover_interrupted(&self) -> AppResult<(u64, u64)> {
        let now = now_millis();
        let mut transaction = self.pool.begin().await?;
        let nodes = sqlx::query(
            "UPDATE workflow_node_runs SET status='uncertain',finished_at=?,duration_ms=CASE WHEN started_at IS NULL THEN 0 ELSE MAX(0,?-started_at) END,error_message='应用退出时远程节点状态未确认',retryable=1 WHERE status='running'",
        )
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let runs = sqlx::query(
            "UPDATE workflow_runs SET status='uncertain',finished_at=?,duration_ms=CASE WHEN started_at IS NULL THEN 0 ELSE MAX(0,?-started_at) END,error_category='interrupted',error_message='应用退出时远程工作流状态未确认',retryable=1 WHERE status='running'",
        )
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        transaction.commit().await?;
        Ok((runs, nodes))
    }

    async fn get_run_record(&self, run_id: Uuid) -> AppResult<Option<WorkflowRunRecord>> {
        sqlx::query(
            "SELECT id,workflow_id,workflow_version,server_id,status,current_node_id,created_at,started_at,finished_at,duration_ms,error_category,error_message,retryable FROM workflow_runs WHERE id=?",
        )
        .bind(run_id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .map(|row| map_run(&row))
        .transpose()
    }

    async fn get_node_attempt(
        &self,
        run_id: Uuid,
        node_id: Uuid,
        attempt: i32,
    ) -> AppResult<Option<WorkflowNodeRun>> {
        sqlx::query(
            "SELECT run_id,node_id,attempt,status,execution_id,started_at,finished_at,duration_ms,exit_code,output_summary,error_message,retryable FROM workflow_node_runs WHERE run_id=? AND node_id=? AND attempt=?",
        )
        .bind(run_id.to_string())
        .bind(node_id.to_string())
        .bind(i64::from(attempt))
        .fetch_optional(&self.pool)
        .await?
        .map(|row| map_node_run(&row))
        .transpose()
    }
}

fn validate_draft(draft: &WorkflowDraft) -> AppResult<()> {
    let name = draft.name.trim();
    if name.is_empty() || name.chars().count() > 200 || draft.description.len() > 4096 {
        return Err(AppError::Validation("工作流名称或说明超过限制".into()));
    }
    if draft.nodes.len() > 100 || draft.edges.len() > 200 {
        return Err(AppError::Validation(
            "工作流图超过 100 个节点或 200 条边".into(),
        ));
    }
    Ok(())
}

fn map_summary(row: &SqliteRow) -> AppResult<WorkflowSummary> {
    Ok(WorkflowSummary {
        id: parse_uuid(row.try_get("id")?)?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        current_version: parse_i32(row.try_get("current_version")?, "工作流版本")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_definition(row: &SqliteRow) -> AppResult<WorkflowDefinition> {
    let definition_json: String = row.try_get("definition_json")?;
    let stored: StoredWorkflowVersion = serde_json::from_str(&definition_json)
        .map_err(|_| AppError::Validation("数据库中的工作流定义损坏".into()))?;
    Ok(WorkflowDefinition {
        id: parse_uuid(row.try_get("id")?)?,
        name: stored.name,
        description: stored.description,
        version: parse_i32(row.try_get("version")?, "工作流版本")?,
        checksum_sha256: row.try_get("checksum_sha256")?,
        nodes: stored.nodes,
        edges: stored.edges,
    })
}

fn map_run(row: &SqliteRow) -> AppResult<WorkflowRunRecord> {
    let status: String = row.try_get("status")?;
    let current_node_id: Option<String> = row.try_get("current_node_id")?;
    let duration_ms: Option<i64> = row.try_get("duration_ms")?;
    Ok(WorkflowRunRecord {
        id: parse_uuid(row.try_get("id")?)?,
        workflow_id: parse_uuid(row.try_get("workflow_id")?)?,
        workflow_version: parse_i32(row.try_get("workflow_version")?, "工作流版本")?,
        server_id: row.try_get("server_id")?,
        status: WorkflowRunStatus::from_str(&status)?,
        current_node_id: current_node_id.map(parse_uuid).transpose()?,
        created_at: row.try_get("created_at")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        duration_ms: duration_ms.map(parse_u64).transpose()?,
        error_category: row.try_get("error_category")?,
        error_message: row.try_get("error_message")?,
        retryable: row.try_get("retryable")?,
    })
}

fn map_node_run(row: &SqliteRow) -> AppResult<WorkflowNodeRun> {
    let status: String = row.try_get("status")?;
    let execution_id: Option<String> = row.try_get("execution_id")?;
    let duration_ms: Option<i64> = row.try_get("duration_ms")?;
    Ok(WorkflowNodeRun {
        run_id: parse_uuid(row.try_get("run_id")?)?,
        node_id: parse_uuid(row.try_get("node_id")?)?,
        attempt: parse_i32(row.try_get("attempt")?, "节点尝试次数")?,
        status: WorkflowNodeStatus::from_str(&status)?,
        execution_id: execution_id.map(parse_uuid).transpose()?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        duration_ms: duration_ms.map(parse_u64).transpose()?,
        exit_code: row.try_get("exit_code")?,
        output_summary: row.try_get("output_summary")?,
        error_message: row.try_get("error_message")?,
        retryable: row.try_get("retryable")?,
    })
}

fn map_event(row: &SqliteRow) -> AppResult<WorkflowRunEvent> {
    let payload_json: String = row.try_get("payload_json")?;
    Ok(WorkflowRunEvent {
        run_id: parse_uuid(row.try_get("run_id")?)?,
        sequence: row.try_get("sequence")?,
        event_type: row.try_get("event_type")?,
        payload: serde_json::from_str(&payload_json)
            .map_err(|_| AppError::Validation("数据库中的工作流事件损坏".into()))?,
        emitted_at: row.try_get("emitted_at")?,
    })
}

fn map_restore_point(row: &SqliteRow) -> AppResult<WorkflowRestorePoint> {
    let status: String = row.try_get("status")?;
    let applicability_json: String = row.try_get("applicability_json")?;
    let size_bytes: Option<i64> = row.try_get("size_bytes")?;
    Ok(WorkflowRestorePoint {
        id: parse_uuid(row.try_get("id")?)?,
        run_id: parse_uuid(row.try_get("run_id")?)?,
        node_id: parse_uuid(row.try_get("node_id")?)?,
        remote_path: row.try_get("remote_path")?,
        relative_path: row.try_get("relative_path")?,
        original_existed: row.try_get("original_existed")?,
        size_bytes: size_bytes.map(parse_u64).transpose()?,
        sha256: row.try_get("sha256")?,
        status: WorkflowRestorePointStatus::from_str(&status)?,
        applicability: serde_json::from_str(&applicability_json)
            .map_err(|_| AppError::Validation("数据库中的恢复点适用条件损坏".into()))?,
        error_message: row.try_get("error_message")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn parse_uuid(value: String) -> AppResult<Uuid> {
    Uuid::parse_str(&value).map_err(|_| AppError::Validation("数据库中的 UUID 无效".into()))
}

fn parse_i32(value: i64, label: &str) -> AppResult<i32> {
    i32::try_from(value).map_err(|_| AppError::Validation(format!("{label}超出范围")))
}

fn parse_u64(value: i64) -> AppResult<u64> {
    u64::try_from(value).map_err(|_| AppError::Validation("数据库中的时长无效".into()))
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

fn ensure_one(rows: u64, message: &str) -> AppResult<()> {
    if rows == 1 {
        Ok(())
    } else {
        Err(AppError::Validation(message.into()))
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
