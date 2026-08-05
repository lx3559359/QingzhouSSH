use std::path::{Component, Path};

use sqlx::{sqlite::SqliteRow, Row, SqlitePool};
use uuid::Uuid;

use crate::{
    core::tasks::BackupItemKind,
    domain::{
        execution::now_millis,
        operation_restore::{
            NewOperationRestoreItem, NewOperationRestorePoint, OperationRestoreDetails,
            OperationRestoreItem, OperationRestoreItemStatus, OperationRestorePoint,
            OperationRestorePointStatus,
        },
    },
    error::{AppError, AppResult},
};

#[derive(Clone)]
pub struct OperationRestoreRepository {
    pool: SqlitePool,
}

impl OperationRestoreRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        draft: NewOperationRestorePoint,
    ) -> AppResult<OperationRestorePoint> {
        validate_point(&draft)?;
        let run = sqlx::query("SELECT server_id,task_id FROM operation_runs WHERE id=?")
            .bind(draft.operation_run_id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| AppError::Validation("运维运行不存在".into()))?;
        if run.try_get::<String, _>("server_id")? != draft.server_id
            || run.try_get::<String, _>("task_id")? != draft.task_id
        {
            return Err(AppError::Validation(
                "恢复点与运维运行的服务器或任务不一致".into(),
            ));
        }

        let id = Uuid::new_v4();
        let now = now_millis();
        sqlx::query(
            "INSERT INTO operation_restore_points (id,operation_run_id,server_id,task_id,status,local_relative_dir,remote_asset_id,expires_at,created_at,updated_at) VALUES (?,?,?,?,'creating',?,?,?,?,?)",
        )
        .bind(id.to_string())
        .bind(draft.operation_run_id.to_string())
        .bind(draft.server_id)
        .bind(draft.task_id)
        .bind(draft.local_relative_dir)
        .bind(draft.remote_asset_id)
        .bind(draft.expires_at)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.get_point(id)
            .await?
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn add_item(
        &self,
        draft: NewOperationRestoreItem,
    ) -> AppResult<OperationRestoreItem> {
        validate_item(&draft)?;
        let point = self
            .get_point(draft.restore_point_id)
            .await?
            .ok_or_else(|| AppError::Validation("运维恢复点不存在".into()))?;
        if point.status != OperationRestorePointStatus::Creating {
            return Err(AppError::Validation(
                "只能向创建中的恢复点添加恢复项".into(),
            ));
        }
        if let Some(relative) = draft.local_relative_path.as_deref() {
            require_descendant(&point.local_relative_dir, relative)?;
        }
        let metadata = serde_json::to_string(&draft.original_metadata)
            .map_err(|error| AppError::Serialization(error.to_string()))?;
        if metadata.len() > 16 * 1024 {
            return Err(AppError::Validation("恢复项原始元数据超过 16 KiB".into()));
        }
        let ordinal = i64::try_from(draft.ordinal)
            .map_err(|_| AppError::Validation("恢复项序号超出范围".into()))?;
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO operation_restore_items (id,restore_point_id,ordinal,item_kind,remote_target,local_relative_path,sha256,original_metadata_json,status,error_summary) VALUES (?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(id.to_string())
        .bind(draft.restore_point_id.to_string())
        .bind(ordinal)
        .bind(item_kind_str(draft.item_kind))
        .bind(draft.remote_target)
        .bind(draft.local_relative_path)
        .bind(draft.sha256.map(|value| value.to_ascii_lowercase()))
        .bind(metadata)
        .bind(draft.status.as_str())
        .bind(cap_utf8(draft.error_summary, 8 * 1024))
        .execute(&self.pool)
        .await?;
        self.get_item(id)
            .await?
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn mark_available(&self, id: Uuid) -> AppResult<()> {
        let result = sqlx::query(
            "UPDATE operation_restore_points SET status='available',updated_at=? WHERE id=? AND status='creating' AND EXISTS (SELECT 1 FROM operation_restore_items WHERE restore_point_id=?) AND NOT EXISTS (SELECT 1 FROM operation_restore_items WHERE restore_point_id=? AND status<>'available')",
        )
        .bind(now_millis())
        .bind(id.to_string())
        .bind(id.to_string())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        ensure_one(
            result.rows_affected(),
            "恢复点尚未完成全部恢复项，不能标记为可用",
        )
    }

    pub async fn attach_remote_asset(
        &self,
        id: Uuid,
        remote_asset_id: &str,
        expires_at: i64,
    ) -> AppResult<()> {
        if remote_asset_id.is_empty()
            || remote_asset_id.len() > 256
            || remote_asset_id.starts_with('/')
            || remote_asset_id.contains(['\\', '\0'])
            || remote_asset_id
                .split('/')
                .any(|segment| segment.is_empty() || segment == "." || segment == "..")
            || expires_at <= 0
        {
            return Err(AppError::Validation("远程恢复资产元数据无效".into()));
        }
        let result = sqlx::query(
            "UPDATE operation_restore_points SET remote_asset_id=?,expires_at=?,updated_at=? WHERE id=? AND status='available' AND remote_asset_id IS NULL",
        )
        .bind(remote_asset_id)
        .bind(expires_at)
        .bind(now_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        ensure_one(result.rows_affected(), "恢复点无法登记远程恢复资产")
    }

    pub async fn mark_creation_failed(&self, id: Uuid) -> AppResult<()> {
        let result = sqlx::query(
            "UPDATE operation_restore_points SET status='failed',updated_at=? WHERE id=? AND status='creating'",
        )
        .bind(now_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        ensure_one(result.rows_affected(), "恢复点不处于创建状态")
    }

    pub async fn mark_item_rolling_back(&self, id: Uuid) -> AppResult<()> {
        let result = sqlx::query(
            "UPDATE operation_restore_items SET status='rolling_back',error_summary=NULL WHERE id=? AND status IN ('available','failed')",
        )
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        ensure_one(result.rows_affected(), "恢复项当前状态不允许回滚")
    }

    pub async fn finish_item_rollback(
        &self,
        id: Uuid,
        status: OperationRestoreItemStatus,
        error_summary: Option<String>,
    ) -> AppResult<()> {
        if !matches!(
            status,
            OperationRestoreItemStatus::RolledBack
                | OperationRestoreItemStatus::Failed
                | OperationRestoreItemStatus::Skipped
        ) {
            return Err(AppError::Validation("恢复项回滚结果状态无效".into()));
        }
        let result = sqlx::query(
            "UPDATE operation_restore_items SET status=?,error_summary=? WHERE id=? AND status='rolling_back'",
        )
        .bind(status.as_str())
        .bind(cap_utf8(error_summary, 8 * 1024))
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        ensure_one(result.rows_affected(), "恢复项不处于回滚执行状态")
    }

    pub async fn recover_interrupted(&self) -> AppResult<u64> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "UPDATE operation_restore_items SET status='failed',error_summary='应用退出时恢复项状态未确认' WHERE status='rolling_back'",
        )
        .execute(&mut *transaction)
        .await?;
        let result = sqlx::query(
            "UPDATE operation_restore_points SET status='partial',updated_at=? WHERE status='rolling_back'",
        )
        .bind(now_millis())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(result.rows_affected())
    }

    pub async fn begin_rollback(&self, id: Uuid) -> AppResult<()> {
        let result = sqlx::query(
            "UPDATE operation_restore_points SET status='rolling_back',updated_at=? WHERE id=? AND status IN ('available','partial','failed')",
        )
        .bind(now_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 1 {
            return Ok(());
        }
        match self.get_point(id).await? {
            Some(point) if point.status == OperationRestorePointStatus::RolledBack => {
                Err(AppError::RestorePointAlreadyConsumed)
            }
            Some(_) => Err(AppError::Validation("恢复点当前状态不允许回滚".into())),
            None => Err(AppError::Validation("运维恢复点不存在".into())),
        }
    }

    pub async fn finish_rollback(
        &self,
        id: Uuid,
        status: OperationRestorePointStatus,
        _error_summary: Option<String>,
    ) -> AppResult<()> {
        if !matches!(
            status,
            OperationRestorePointStatus::RolledBack
                | OperationRestorePointStatus::Partial
                | OperationRestorePointStatus::Failed
        ) {
            return Err(AppError::Validation("恢复点回滚结果状态无效".into()));
        }
        let result = sqlx::query(
            "UPDATE operation_restore_points SET status=?,updated_at=? WHERE id=? AND status='rolling_back'",
        )
        .bind(status.as_str())
        .bind(now_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        ensure_one(result.rows_affected(), "恢复点不处于回滚执行状态")
    }

    pub async fn get(&self, id: Uuid) -> AppResult<Option<OperationRestoreDetails>> {
        let Some(point) = self.get_point(id).await? else {
            return Ok(None);
        };
        let items = self.list_items(id).await?;
        Ok(Some(OperationRestoreDetails { point, items }))
    }

    pub async fn list_by_run(
        &self,
        operation_run_id: Uuid,
    ) -> AppResult<Vec<OperationRestoreDetails>> {
        let points = sqlx::query(
            "SELECT id,operation_run_id,server_id,task_id,status,local_relative_dir,remote_asset_id,expires_at,created_at,updated_at FROM operation_restore_points WHERE operation_run_id=? ORDER BY created_at,id",
        )
        .bind(operation_run_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(map_point)
        .collect::<AppResult<Vec<_>>>()?;
        let mut details = Vec::with_capacity(points.len());
        for point in points {
            let items = self.list_items(point.id).await?;
            details.push(OperationRestoreDetails { point, items });
        }
        Ok(details)
    }

    async fn get_point(&self, id: Uuid) -> AppResult<Option<OperationRestorePoint>> {
        sqlx::query(
            "SELECT id,operation_run_id,server_id,task_id,status,local_relative_dir,remote_asset_id,expires_at,created_at,updated_at FROM operation_restore_points WHERE id=?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .map(|row| map_point(&row))
        .transpose()
    }

    async fn get_item(&self, id: Uuid) -> AppResult<Option<OperationRestoreItem>> {
        sqlx::query(
            "SELECT id,restore_point_id,ordinal,item_kind,remote_target,local_relative_path,sha256,original_metadata_json,status,error_summary FROM operation_restore_items WHERE id=?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .map(|row| map_item(&row))
        .transpose()
    }

    async fn list_items(&self, point_id: Uuid) -> AppResult<Vec<OperationRestoreItem>> {
        sqlx::query(
            "SELECT id,restore_point_id,ordinal,item_kind,remote_target,local_relative_path,sha256,original_metadata_json,status,error_summary FROM operation_restore_items WHERE restore_point_id=? ORDER BY ordinal,id",
        )
        .bind(point_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(map_item)
        .collect()
    }
}

fn validate_point(draft: &NewOperationRestorePoint) -> AppResult<()> {
    if draft.server_id.trim().is_empty()
        || draft.task_id.trim().is_empty()
        || draft.server_id.contains('\0')
        || draft.task_id.contains('\0')
        || draft.remote_asset_id.as_ref().is_some_and(|value| {
            value.contains('\0') || value.len() > 512 || value.trim().is_empty()
        })
    {
        return Err(AppError::Validation("运维恢复点定义无效".into()));
    }
    validate_relative_path(&draft.local_relative_dir)
}

fn validate_item(draft: &NewOperationRestoreItem) -> AppResult<()> {
    if draft.ordinal >= 1000
        || draft.remote_target.trim().is_empty()
        || draft.remote_target.contains('\0')
        || draft.remote_target.len() > 4096
        || draft
            .error_summary
            .as_ref()
            .is_some_and(|value| value.contains('\0'))
    {
        return Err(AppError::Validation("运维恢复项定义无效".into()));
    }
    if let Some(relative) = draft.local_relative_path.as_deref() {
        validate_relative_path(relative)?;
        if relative.ends_with(".partial") {
            return Err(AppError::Security("临时恢复资产不能登记为可用备份".into()));
        }
    }
    if draft
        .sha256
        .as_deref()
        .is_some_and(|hash| hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(AppError::Validation("恢复项 SHA-256 无效".into()));
    }
    Ok(())
}

fn validate_relative_path(value: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 4096
        || value.contains('\0')
        || value.contains('\\')
        || Path::new(value).components().any(|component| {
            !matches!(component, Component::Normal(_))
                || component.as_os_str().to_string_lossy().contains(':')
        })
    {
        return Err(AppError::Security(
            "恢复资产路径必须是项目数据目录内的安全相对路径".into(),
        ));
    }
    Ok(())
}

fn require_descendant(parent: &str, child: &str) -> AppResult<()> {
    let parent = Path::new(parent);
    let child = Path::new(child);
    if child.starts_with(parent) && child != parent {
        Ok(())
    } else {
        Err(AppError::Security("恢复项路径不在恢复点目录内".into()))
    }
}

fn map_point(row: &SqliteRow) -> AppResult<OperationRestorePoint> {
    let status: String = row.try_get("status")?;
    Ok(OperationRestorePoint {
        id: parse_uuid(row.try_get("id")?, "恢复点")?,
        operation_run_id: parse_uuid(row.try_get("operation_run_id")?, "运维运行")?,
        server_id: row.try_get("server_id")?,
        task_id: row.try_get("task_id")?,
        status: OperationRestorePointStatus::try_from(status.as_str())?,
        local_relative_dir: row.try_get("local_relative_dir")?,
        remote_asset_id: row.try_get("remote_asset_id")?,
        expires_at: row.try_get("expires_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_item(row: &SqliteRow) -> AppResult<OperationRestoreItem> {
    let kind: String = row.try_get("item_kind")?;
    let status: String = row.try_get("status")?;
    let metadata: String = row.try_get("original_metadata_json")?;
    let ordinal = usize::try_from(row.try_get::<i64, _>("ordinal")?)
        .map_err(|_| AppError::Validation("数据库中的恢复项序号无效".into()))?;
    Ok(OperationRestoreItem {
        id: parse_uuid(row.try_get("id")?, "恢复项")?,
        restore_point_id: parse_uuid(row.try_get("restore_point_id")?, "恢复点")?,
        ordinal,
        item_kind: parse_item_kind(&kind)?,
        remote_target: row.try_get("remote_target")?,
        local_relative_path: row.try_get("local_relative_path")?,
        sha256: row.try_get("sha256")?,
        original_metadata: serde_json::from_str(&metadata)
            .map_err(|_| AppError::Validation("数据库中的恢复项元数据损坏".into()))?,
        status: OperationRestoreItemStatus::try_from(status.as_str())?,
        error_summary: row.try_get("error_summary")?,
    })
}

fn item_kind_str(kind: BackupItemKind) -> &'static str {
    match kind {
        BackupItemKind::RemoteFile => "remote_file",
        BackupItemKind::CommandSnapshot => "command_snapshot",
        BackupItemKind::ManagedBlock => "managed_block",
        BackupItemKind::RuntimeState => "runtime_state",
    }
}

fn parse_item_kind(value: &str) -> AppResult<BackupItemKind> {
    match value {
        "remote_file" => Ok(BackupItemKind::RemoteFile),
        "command_snapshot" => Ok(BackupItemKind::CommandSnapshot),
        "managed_block" => Ok(BackupItemKind::ManagedBlock),
        "runtime_state" => Ok(BackupItemKind::RuntimeState),
        other => Err(AppError::Validation(format!(
            "数据库中的恢复项类型无效：{other}"
        ))),
    }
}

fn parse_uuid(value: String, label: &str) -> AppResult<Uuid> {
    Uuid::parse_str(&value)
        .map_err(|_| AppError::Validation(format!("数据库中的{label} UUID 无效")))
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
