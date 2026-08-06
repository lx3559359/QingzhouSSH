use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    core::{
        data_migration::{
            preflight_data_root_migration, preflight_retry_data_root_migration,
            DataMigrationJournal, DataMigrationPhase, DataMigrationPreview, MigrationJournalStore,
            MIGRATION_JOURNAL_FILE,
        },
        data_root::DataRootSource,
    },
    domain::execution::now_millis,
    error::{AppError, AppResult},
};

#[derive(Debug, Clone)]
struct StoredPreview {
    preview: DataMigrationPreview,
    source_mode: DataRootSource,
}

#[derive(Clone, Default)]
pub struct DataMigrationPreviewRegistry {
    previews: Arc<Mutex<HashMap<Uuid, StoredPreview>>>,
}

impl DataMigrationPreviewRegistry {
    pub async fn issue(
        &self,
        preview: DataMigrationPreview,
        source_mode: DataRootSource,
    ) -> DataMigrationPreview {
        self.previews.lock().await.insert(
            preview.preview_id,
            StoredPreview {
                preview: preview.clone(),
                source_mode,
            },
        );
        preview
    }

    pub async fn consume(
        &self,
        preview_id: Uuid,
        confirmation_token: Uuid,
        now: i64,
    ) -> AppResult<(DataMigrationPreview, DataRootSource)> {
        let mut previews = self.previews.lock().await;
        let stored = previews
            .get(&preview_id)
            .ok_or_else(|| AppError::Validation("数据目录迁移预检不存在或已经使用".into()))?;
        if stored.preview.expires_at <= now {
            previews.remove(&preview_id);
            return Err(AppError::Validation("数据目录迁移确认已过期".into()));
        }
        if stored.preview.confirmation_token != confirmation_token {
            return Err(AppError::Security("数据目录迁移确认令牌无效".into()));
        }
        let stored = previews
            .remove(&preview_id)
            .ok_or_else(|| AppError::Validation("数据目录迁移预检已经使用".into()))?;
        Ok((stored.preview, stored.source_mode))
    }
}

pub trait MigrationWorkerSpawner: Send + Sync {
    fn spawn(&self, journal_path: &Path) -> AppResult<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemMigrationWorkerSpawner;

impl MigrationWorkerSpawner for SystemMigrationWorkerSpawner {
    fn spawn(&self, journal_path: &Path) -> AppResult<()> {
        let executable = std::env::current_exe()?;
        Command::new(executable)
            .arg("--migrate-data-root")
            .arg(journal_path)
            .spawn()?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct DataMigrationService {
    data_root: PathBuf,
    registry: DataMigrationPreviewRegistry,
    spawner: Arc<dyn MigrationWorkerSpawner>,
}

impl DataMigrationService {
    pub fn new(data_root: PathBuf) -> Self {
        Self::new_with_spawner(data_root, Arc::new(SystemMigrationWorkerSpawner))
    }

    pub fn new_with_spawner(data_root: PathBuf, spawner: Arc<dyn MigrationWorkerSpawner>) -> Self {
        Self {
            data_root,
            registry: DataMigrationPreviewRegistry::default(),
            spawner,
        }
    }

    pub async fn preflight(
        &self,
        target: &Path,
        source_mode: DataRootSource,
        mutable: bool,
    ) -> AppResult<DataMigrationPreview> {
        ensure_mutable_source(source_mode, mutable)?;
        let (preview, _) = preflight_data_root_migration(&self.data_root, target, now_millis())?;
        Ok(self.registry.issue(preview, source_mode).await)
    }

    pub async fn preflight_retry(
        &self,
        source_mode: DataRootSource,
        mutable: bool,
    ) -> AppResult<DataMigrationPreview> {
        ensure_mutable_source(source_mode, mutable)?;
        let failed = self
            .status()?
            .filter(|journal| journal.phase == DataMigrationPhase::Failed)
            .ok_or_else(|| AppError::Validation("没有可重试的失败迁移".into()))?;
        if std::fs::canonicalize(&failed.source)? != std::fs::canonicalize(&self.data_root)?
            || failed.source_mode != source_mode
        {
            return Err(AppError::Security("失败迁移不属于当前数据目录".into()));
        }
        let (preview, _) =
            preflight_retry_data_root_migration(&self.data_root, &failed.target, now_millis())?;
        Ok(self.registry.issue(preview, source_mode).await)
    }

    pub async fn start(
        &self,
        preview_id: Uuid,
        confirmation_token: Uuid,
        source_mode: DataRootSource,
        mutable: bool,
    ) -> AppResult<DataMigrationJournal> {
        ensure_mutable_source(source_mode, mutable)?;
        let (stored_preview, stored_source_mode) = self
            .registry
            .consume(preview_id, confirmation_token, now_millis())
            .await?;
        if stored_source_mode != source_mode {
            return Err(AppError::Security("数据目录来源在确认前发生变化".into()));
        }
        let (current, manifest) = if stored_preview.retryable {
            preflight_retry_data_root_migration(
                &self.data_root,
                &stored_preview.target,
                now_millis(),
            )?
        } else {
            preflight_data_root_migration(&self.data_root, &stored_preview.target, now_millis())?
        };
        if current.source != stored_preview.source
            || current.target != stored_preview.target
            || current.file_count != stored_preview.file_count
            || current.total_bytes != stored_preview.total_bytes
        {
            return Err(AppError::Security(
                "数据目录或目标状态在确认前发生变化，请重新预检".into(),
            ));
        }
        let journal = DataMigrationJournal::prepared(
            current.source,
            current.target,
            source_mode,
            std::process::id(),
            &manifest,
            now_millis(),
        );
        let store = MigrationJournalStore::new(&journal.source, &journal.target);
        store.save(&journal)?;
        self.spawner.spawn(store.source_path())?;
        Ok(journal)
    }

    pub fn status(&self) -> AppResult<Option<DataMigrationJournal>> {
        let path = self.data_root.join(MIGRATION_JOURNAL_FILE);
        if !path.is_file() {
            return Ok(None);
        }
        MigrationJournalStore::load(&path).map(Some)
    }

    pub fn acknowledge(&self, migration_id: Uuid) -> AppResult<DataMigrationJournal> {
        let mut journal = self
            .status()?
            .ok_or_else(|| AppError::Validation("没有可确认的数据迁移结果".into()))?;
        if journal.migration_id != migration_id {
            return Err(AppError::Security("数据迁移结果标识不匹配".into()));
        }
        journal.acknowledged = true;
        journal.updated_at = now_millis();
        MigrationJournalStore::new(&journal.source, &journal.target).save(&journal)?;
        Ok(journal)
    }

    pub fn open_current_folder(&self) -> AppResult<()> {
        open_folder(&self.data_root)
    }

    pub fn open_last_source_folder(&self) -> AppResult<()> {
        let journal = self
            .status()?
            .ok_or_else(|| AppError::Validation("没有上次迁移的旧目录".into()))?;
        open_folder(&journal.source)
    }
}

fn ensure_mutable_source(source_mode: DataRootSource, mutable: bool) -> AppResult<()> {
    if !mutable || source_mode == DataRootSource::Environment {
        return Err(AppError::Validation(
            "数据目录由 QINGZHOU_DATA_ROOT 环境变量锁定，客户端不能修改".into(),
        ));
    }
    if source_mode == DataRootSource::NeedsSelection {
        return Err(AppError::NotReady);
    }
    Ok(())
}

fn open_folder(path: &Path) -> AppResult<()> {
    if !path.is_absolute() || !path.is_dir() {
        return Err(AppError::Validation("要打开的数据目录无效".into()));
    }
    Command::new("explorer.exe").arg(path).spawn()?;
    Ok(())
}
