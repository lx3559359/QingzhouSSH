use sha2::{Digest, Sha256};
use sqlx::{sqlite::SqliteRow, QueryBuilder, Row, Sqlite, SqlitePool};
use uuid::Uuid;

use crate::{
    core::{
        scripts::validation::{
            scan_script_body_for, validate_script_metadata, validate_script_parameters,
            validate_script_timeout, ScriptScanSummary,
        },
        tasks::ParameterDefinition,
    },
    domain::{
        execution::now_millis,
        script::{
            NewPersonalScript, NewScriptRunReference, NewScriptVersion, ScriptDefinition,
            ScriptDetails, ScriptListFilter, ScriptMetadataUpdate, ScriptRunReference,
            ScriptSummary, ScriptVersion,
        },
    },
    error::{AppError, AppResult},
};

#[derive(Clone)]
pub struct ScriptRepository {
    pool: SqlitePool,
}

impl ScriptRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, draft: NewPersonalScript) -> AppResult<ScriptDetails> {
        validate_definition(&draft)?;
        let scan = validate_version(&draft.version)?;
        let definition_id = Uuid::new_v4();
        let version_id = Uuid::new_v4();
        let now = now_millis();
        let tags_json = to_bounded_json(&draft.tags, 8 * 1024, "脚本标签")?;
        let parameters_json = to_bounded_json(&draft.version.parameters, 128 * 1024, "脚本参数")?;
        let scan_summary_json = to_bounded_json(&scan, 64 * 1024, "脚本扫描摘要")?;
        let compatibility_json =
            to_bounded_json(&draft.version.compatibility, 16 * 1024, "脚本兼容性")?;
        let body_sha256 = sha256(&draft.version.body);
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO script_definitions (id,title,category,tags_json,is_favorite,is_enabled,active_version_id,created_at,updated_at,deleted_at) VALUES (?,?,?,?,?,?,NULL,?,?,NULL)",
        )
        .bind(definition_id.to_string())
        .bind(draft.title)
        .bind(draft.category)
        .bind(tags_json)
        .bind(bool_i64(draft.is_favorite))
        .bind(bool_i64(draft.is_enabled))
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO script_versions (id,definition_id,version_number,body,body_sha256,parameters_json,scan_summary_json,timeout_seconds,shell,compatibility_json,created_at) VALUES (?,?,1,?,?,?,?,?,?,?,?)",
        )
        .bind(version_id.to_string())
        .bind(definition_id.to_string())
        .bind(draft.version.body)
        .bind(body_sha256)
        .bind(parameters_json)
        .bind(scan_summary_json)
        .bind(i64::try_from(draft.version.timeout_seconds).map_err(|_| {
            AppError::Validation("脚本超时时间超出数据库范围".into())
        })?)
        .bind(draft.version.shell.as_str())
        .bind(compatibility_json)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("UPDATE script_definitions SET active_version_id=? WHERE id=?")
            .bind(version_id.to_string())
            .bind(definition_id.to_string())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        self.get_for_editor(definition_id)
            .await?
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn save_version(
        &self,
        definition_id: Uuid,
        draft: NewScriptVersion,
    ) -> AppResult<ScriptVersion> {
        let scan = validate_version(&draft)?;
        let parameters_json = to_bounded_json(&draft.parameters, 128 * 1024, "脚本参数")?;
        let scan_summary_json = to_bounded_json(&scan, 64 * 1024, "脚本扫描摘要")?;
        let compatibility_json = to_bounded_json(&draft.compatibility, 16 * 1024, "脚本兼容性")?;
        let body_sha256 = sha256(&draft.body);
        let version_id = Uuid::new_v4();
        let now = now_millis();
        let mut transaction = self.pool.begin().await?;
        let next_version: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(v.version_number),0)+1 FROM script_versions v JOIN script_definitions d ON d.id=v.definition_id WHERE d.id=? AND d.deleted_at IS NULL",
        )
        .bind(definition_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        if next_version <= 1 {
            return Err(AppError::Validation("脚本不存在或已删除".into()));
        }
        sqlx::query(
            "INSERT INTO script_versions (id,definition_id,version_number,body,body_sha256,parameters_json,scan_summary_json,timeout_seconds,shell,compatibility_json,created_at) VALUES (?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(version_id.to_string())
        .bind(definition_id.to_string())
        .bind(next_version)
        .bind(draft.body)
        .bind(body_sha256)
        .bind(parameters_json)
        .bind(scan_summary_json)
        .bind(i64::try_from(draft.timeout_seconds).map_err(|_| {
            AppError::Validation("脚本超时时间超出数据库范围".into())
        })?)
        .bind(draft.shell.as_str())
        .bind(compatibility_json)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        let updated = sqlx::query(
            "UPDATE script_definitions SET active_version_id=?,updated_at=? WHERE id=? AND deleted_at IS NULL",
        )
        .bind(version_id.to_string())
        .bind(now)
        .bind(definition_id.to_string())
        .execute(&mut *transaction)
        .await?;
        ensure_one(updated.rows_affected(), "脚本不存在或已删除")?;
        transaction.commit().await?;
        self.get_version_by_id(version_id)
            .await?
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn get_for_editor(&self, id: Uuid) -> AppResult<Option<ScriptDetails>> {
        let row = sqlx::query(
            "SELECT d.id,d.title,d.category,d.tags_json,d.is_favorite,d.is_enabled,d.active_version_id,d.created_at,d.updated_at,d.deleted_at,v.id AS version_id,v.definition_id,v.version_number,v.body,v.body_sha256,v.parameters_json,v.scan_summary_json,v.timeout_seconds,v.shell,v.compatibility_json,v.created_at AS version_created_at FROM script_definitions d JOIN script_versions v ON v.id=d.active_version_id AND v.definition_id=d.id WHERE d.id=? AND d.deleted_at IS NULL",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(ScriptDetails {
                definition: map_definition(&row)?,
                active_version: map_joined_version(&row)?,
            })
        })
        .transpose()
    }

    pub async fn get_version(
        &self,
        definition_id: Uuid,
        version_number: u32,
    ) -> AppResult<ScriptVersion> {
        sqlx::query(
            "SELECT id,definition_id,version_number,body,body_sha256,parameters_json,scan_summary_json,timeout_seconds,shell,compatibility_json,created_at FROM script_versions WHERE definition_id=? AND version_number=?",
        )
        .bind(definition_id.to_string())
        .bind(i64::from(version_number))
        .fetch_optional(&self.pool)
        .await?
        .map(|row| map_version(&row))
        .transpose()?
        .ok_or_else(|| AppError::Validation("脚本版本不存在".into()))
    }

    pub async fn list_versions(&self, definition_id: Uuid) -> AppResult<Vec<ScriptVersion>> {
        sqlx::query(
            "SELECT id,definition_id,version_number,body,body_sha256,parameters_json,scan_summary_json,timeout_seconds,shell,compatibility_json,created_at FROM script_versions WHERE definition_id=? ORDER BY version_number DESC LIMIT 100",
        )
        .bind(definition_id.to_string())
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(map_version)
        .collect()
    }

    pub async fn list(&self, filter: ScriptListFilter) -> AppResult<Vec<ScriptSummary>> {
        validate_filter(&filter)?;
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT d.id,d.title,d.category,d.tags_json,d.is_favorite,d.is_enabled,d.active_version_id,d.updated_at,v.version_number,v.body_sha256,v.shell,v.compatibility_json FROM script_definitions d JOIN script_versions v ON v.id=d.active_version_id AND v.definition_id=d.id WHERE d.deleted_at IS NULL",
        );
        if let Some(value) = filter.query.as_deref() {
            query
                .push(" AND (d.title LIKE ")
                .push_bind(format!("%{value}%"));
            query
                .push(" OR d.category LIKE ")
                .push_bind(format!("%{value}%"));
            query.push(")");
        }
        if let Some(value) = filter.category {
            query.push(" AND d.category=").push_bind(value);
        }
        if let Some(value) = filter.tag {
            query
                .push(" AND EXISTS (SELECT 1 FROM json_each(d.tags_json) WHERE value=")
                .push_bind(value)
                .push(")");
        }
        if let Some(value) = filter.favorite {
            query.push(" AND d.is_favorite=").push_bind(bool_i64(value));
        }
        if let Some(value) = filter.enabled {
            query.push(" AND d.is_enabled=").push_bind(bool_i64(value));
        }
        query.push(" ORDER BY d.updated_at DESC,d.id LIMIT 100");
        query
            .build()
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(map_summary)
            .collect()
    }

    pub async fn set_favorite(&self, id: Uuid, favorite: bool) -> AppResult<()> {
        self.set_flag(id, "is_favorite", favorite).await
    }

    pub async fn update_metadata(&self, id: Uuid, update: ScriptMetadataUpdate) -> AppResult<()> {
        validate_script_metadata(&update.title, &update.category, &update.tags)?;
        let tags = to_bounded_json(&update.tags, 8 * 1024, "脚本标签")?;
        let result = sqlx::query(
            "UPDATE script_definitions SET title=?,category=?,tags_json=?,updated_at=? WHERE id=? AND deleted_at IS NULL",
        )
        .bind(update.title)
        .bind(update.category)
        .bind(tags)
        .bind(now_millis())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        ensure_one(result.rows_affected(), "脚本不存在或已删除")
    }

    pub async fn set_enabled(&self, id: Uuid, enabled: bool) -> AppResult<()> {
        self.set_flag(id, "is_enabled", enabled).await
    }

    async fn set_flag(&self, id: Uuid, column: &str, value: bool) -> AppResult<()> {
        let sql = match column {
            "is_favorite" => {
                "UPDATE script_definitions SET is_favorite=?,updated_at=? WHERE id=? AND deleted_at IS NULL"
            }
            "is_enabled" => {
                "UPDATE script_definitions SET is_enabled=?,updated_at=? WHERE id=? AND deleted_at IS NULL"
            }
            _ => return Err(AppError::Validation("脚本标志字段无效".into())),
        };
        let result = sqlx::query(sql)
            .bind(bool_i64(value))
            .bind(now_millis())
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        ensure_one(result.rows_affected(), "脚本不存在或已删除")
    }

    pub async fn soft_delete(&self, id: Uuid) -> AppResult<()> {
        let now = now_millis();
        let result = sqlx::query(
            "UPDATE script_definitions SET deleted_at=?,is_enabled=0,updated_at=? WHERE id=? AND deleted_at IS NULL",
        )
        .bind(now)
        .bind(now)
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        ensure_one(result.rows_affected(), "脚本不存在或已经删除")
    }

    pub async fn record_run(&self, draft: NewScriptRunReference) -> AppResult<ScriptRunReference> {
        let valid: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM script_versions v JOIN script_definitions d ON d.id=v.definition_id JOIN operation_runs o ON o.id=? WHERE v.id=? AND v.definition_id=? AND o.task_id='script.personal'",
        )
        .bind(draft.operation_run_id.to_string())
        .bind(draft.version_id.to_string())
        .bind(draft.definition_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        if valid != 1 {
            return Err(AppError::Validation(
                "脚本运行引用与脚本版本或运维运行不匹配".into(),
            ));
        }
        let id = Uuid::new_v4();
        let now = now_millis();
        sqlx::query(
            "INSERT INTO script_runs (id,definition_id,version_id,operation_run_id,created_at) VALUES (?,?,?,?,?)",
        )
        .bind(id.to_string())
        .bind(draft.definition_id.to_string())
        .bind(draft.version_id.to_string())
        .bind(draft.operation_run_id.to_string())
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.get_run(id)
            .await?
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn get_run(&self, id: Uuid) -> AppResult<Option<ScriptRunReference>> {
        sqlx::query(
            "SELECT id,definition_id,version_id,operation_run_id,created_at FROM script_runs WHERE id=?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .map(|row| map_run(&row))
        .transpose()
    }

    async fn get_version_by_id(&self, id: Uuid) -> AppResult<Option<ScriptVersion>> {
        sqlx::query(
            "SELECT id,definition_id,version_number,body,body_sha256,parameters_json,scan_summary_json,timeout_seconds,shell,compatibility_json,created_at FROM script_versions WHERE id=?",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .map(|row| map_version(&row))
        .transpose()
    }
}

fn validate_definition(draft: &NewPersonalScript) -> AppResult<()> {
    validate_script_metadata(&draft.title, &draft.category, &draft.tags)
}

fn validate_version(draft: &NewScriptVersion) -> AppResult<ScriptScanSummary> {
    validate_script_timeout(draft.timeout_seconds)?;
    if draft.compatibility != crate::domain::script::ScriptCompatibility::for_shell(draft.shell) {
        return Err(AppError::Validation(
            "脚本兼容性声明必须与版本 Shell 一致".into(),
        ));
    }
    let parameters: Vec<ParameterDefinition> = serde_json::from_value(draft.parameters.clone())
        .map_err(|_| AppError::Validation("脚本参数定义格式无效".into()))?;
    validate_script_parameters(&parameters)?;
    scan_script_body_for(draft.shell, &draft.body)
}

fn validate_filter(filter: &ScriptListFilter) -> AppResult<()> {
    if [
        filter.query.as_deref(),
        filter.category.as_deref(),
        filter.tag.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.len() > 240 || value.contains('\0'))
    {
        return Err(AppError::Validation("脚本筛选条件无效".into()));
    }
    Ok(())
}

fn to_bounded_json<T: serde::Serialize>(value: &T, limit: usize, name: &str) -> AppResult<String> {
    let encoded =
        serde_json::to_string(value).map_err(|error| AppError::Serialization(error.to_string()))?;
    if encoded.len() > limit {
        return Err(AppError::Validation(format!("{name}超过大小上限")));
    }
    Ok(encoded)
}

fn sha256(body: &str) -> String {
    format!("{:x}", Sha256::digest(body.as_bytes()))
}

fn bool_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn map_definition(row: &SqliteRow) -> AppResult<ScriptDefinition> {
    Ok(ScriptDefinition {
        id: parse_uuid(row.try_get("id")?, "脚本")?,
        title: row.try_get("title")?,
        category: row.try_get("category")?,
        tags: parse_json(row.try_get("tags_json")?, "脚本标签")?,
        is_favorite: parse_bool(row.try_get("is_favorite")?, "脚本收藏状态")?,
        is_enabled: parse_bool(row.try_get("is_enabled")?, "脚本启用状态")?,
        active_version_id: parse_uuid(row.try_get("active_version_id")?, "脚本当前版本")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        deleted_at: row.try_get("deleted_at")?,
    })
}

fn map_version(row: &SqliteRow) -> AppResult<ScriptVersion> {
    let number = u32::try_from(row.try_get::<i64, _>("version_number")?)
        .map_err(|_| AppError::Integrity("脚本版本号无效".into()))?;
    Ok(ScriptVersion {
        id: parse_uuid(row.try_get("id")?, "脚本版本")?,
        definition_id: parse_uuid(row.try_get("definition_id")?, "脚本")?,
        version_number: number,
        body: row.try_get("body")?,
        body_sha256: row.try_get("body_sha256")?,
        parameters: parse_json(row.try_get("parameters_json")?, "脚本参数")?,
        scan_summary: parse_json(row.try_get("scan_summary_json")?, "脚本扫描摘要")?,
        timeout_seconds: parse_timeout(row.try_get("timeout_seconds")?)?,
        shell: crate::domain::script::ScriptShell::try_from(row.try_get::<&str, _>("shell")?)?,
        compatibility: parse_json(row.try_get("compatibility_json")?, "脚本兼容性")?,
        created_at: row.try_get("created_at")?,
    })
}

fn map_joined_version(row: &SqliteRow) -> AppResult<ScriptVersion> {
    let number = u32::try_from(row.try_get::<i64, _>("version_number")?)
        .map_err(|_| AppError::Integrity("脚本版本号无效".into()))?;
    Ok(ScriptVersion {
        id: parse_uuid(row.try_get("version_id")?, "脚本版本")?,
        definition_id: parse_uuid(row.try_get("definition_id")?, "脚本")?,
        version_number: number,
        body: row.try_get("body")?,
        body_sha256: row.try_get("body_sha256")?,
        parameters: parse_json(row.try_get("parameters_json")?, "脚本参数")?,
        scan_summary: parse_json(row.try_get("scan_summary_json")?, "脚本扫描摘要")?,
        timeout_seconds: parse_timeout(row.try_get("timeout_seconds")?)?,
        shell: crate::domain::script::ScriptShell::try_from(row.try_get::<&str, _>("shell")?)?,
        compatibility: parse_json(row.try_get("compatibility_json")?, "脚本兼容性")?,
        created_at: row.try_get("version_created_at")?,
    })
}

fn map_summary(row: &SqliteRow) -> AppResult<ScriptSummary> {
    Ok(ScriptSummary {
        id: parse_uuid(row.try_get("id")?, "脚本")?,
        title: row.try_get("title")?,
        category: row.try_get("category")?,
        tags: parse_json(row.try_get("tags_json")?, "脚本标签")?,
        is_favorite: parse_bool(row.try_get("is_favorite")?, "脚本收藏状态")?,
        is_enabled: parse_bool(row.try_get("is_enabled")?, "脚本启用状态")?,
        active_version_id: parse_uuid(row.try_get("active_version_id")?, "脚本当前版本")?,
        active_version_number: u32::try_from(row.try_get::<i64, _>("version_number")?)
            .map_err(|_| AppError::Integrity("脚本版本号无效".into()))?,
        body_sha256: row.try_get("body_sha256")?,
        shell: crate::domain::script::ScriptShell::try_from(row.try_get::<&str, _>("shell")?)?,
        compatibility: parse_json(row.try_get("compatibility_json")?, "脚本兼容性")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_run(row: &SqliteRow) -> AppResult<ScriptRunReference> {
    Ok(ScriptRunReference {
        id: parse_uuid(row.try_get("id")?, "脚本运行")?,
        definition_id: parse_uuid(row.try_get("definition_id")?, "脚本")?,
        version_id: parse_uuid(row.try_get("version_id")?, "脚本版本")?,
        operation_run_id: parse_uuid(row.try_get("operation_run_id")?, "运维运行")?,
        created_at: row.try_get("created_at")?,
    })
}

fn parse_uuid(value: String, name: &str) -> AppResult<Uuid> {
    Uuid::parse_str(&value).map_err(|_| AppError::Integrity(format!("数据库中的{name}标识无效")))
}

fn parse_json<T: serde::de::DeserializeOwned>(value: String, name: &str) -> AppResult<T> {
    serde_json::from_str(&value)
        .map_err(|_| AppError::Integrity(format!("数据库中的{name}格式无效")))
}

fn parse_bool(value: i64, name: &str) -> AppResult<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(AppError::Integrity(format!("数据库中的{name}无效"))),
    }
}

fn parse_timeout(value: i64) -> AppResult<u64> {
    let value = u64::try_from(value)
        .map_err(|_| AppError::Integrity("数据库中的脚本超时时间无效".into()))?;
    validate_script_timeout(value)?;
    Ok(value)
}

fn ensure_one(rows: u64, message: &str) -> AppResult<()> {
    if rows == 1 {
        Ok(())
    } else {
        Err(AppError::Validation(message.into()))
    }
}
