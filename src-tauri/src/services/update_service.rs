use std::{
    fs::{self, OpenOptions},
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use tauri_plugin_updater::{Update, UpdaterExt};
use thiserror::Error;
use tokio::sync::Mutex;
use url::Url;

use crate::{
    core::updates::{
        parse_manifest, DualSourceChecker, HttpManifestTransport, ManifestDecision,
        SourceCheckError, SourceFailureKind, StoredCheckResult, StoredCheckStatus,
        TrustedSourcePolicy, UpdateChecker, UpdateStateStore, UpdateStateStoreError,
    },
    domain::update::{
        UpdateLifecycle, UpdatePhase, UpdateRelease, UpdateSource, UpdateTransitionError,
    },
};

pub type ProgressCallback = Box<dyn FnMut(u64, Option<u64>) + Send>;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UpdateAdapterError {
    #[error("更新服务网络不可达")]
    Network,
    #[error("更新签名验证失败")]
    Signature,
    #[error("更新清单与待下载版本不一致")]
    Manifest,
    #[error("更新安装器启动失败")]
    Install,
}

pub trait SignedUpdateAdapter: Send + Sync {
    fn download<'a>(
        &'a self,
        release: &'a UpdateRelease,
        progress: ProgressCallback,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, UpdateAdapterError>> + Send + 'a>>;

    fn install<'a>(
        &'a self,
        release: &'a UpdateRelease,
        bytes: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), UpdateAdapterError>> + Send + 'a>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedUpdate {
    pub version: String,
    pub relative_path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUpdate {
    pub version: String,
    pub notes: String,
    pub published_at: Option<String>,
    pub size: u64,
    pub build_id: String,
    pub source: UpdateSource,
    pub source_label: String,
}

impl From<&UpdateRelease> for AvailableUpdate {
    fn from(release: &UpdateRelease) -> Self {
        Self {
            version: release.version.clone(),
            notes: release.notes.clone(),
            published_at: release.published_at.clone(),
            size: release.size,
            build_id: release.build_id.clone(),
            source: release.source,
            source_label: source_label(release.source).into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub phase: UpdatePhase,
    pub auto_check: bool,
    pub last_checked_at: Option<u64>,
    pub last_result: Option<StoredCheckResult>,
    pub release: Option<AvailableUpdate>,
    pub fallback_reason: Option<String>,
    pub staged: Option<StagedUpdate>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgressEvent {
    pub sequence: u64,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Error)]
pub enum UpdateServiceError {
    #[error("{0}")]
    Adapter(#[from] UpdateAdapterError),
    #[error("更新包完整性校验失败")]
    Integrity,
    #[error("更新包尚未完成下载")]
    NotDownloaded,
    #[error("安装更新前必须明确确认")]
    ConfirmationRequired,
    #[error("更新存储失败：{0}")]
    Store(#[from] UpdateStateStoreError),
    #[error("更新文件操作失败：{0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum UpdateManagerError {
    #[error("更新源检查失败：{0}")]
    Source(#[from] SourceCheckError),
    #[error("更新状态转换失败：{0}")]
    Transition(#[from] UpdateTransitionError),
    #[error("更新状态存储失败：{0}")]
    Store(#[from] UpdateStateStoreError),
    #[error("更新处理失败：{0}")]
    Service(#[from] UpdateServiceError),
}

#[derive(Debug, Clone)]
struct PendingUpdate {
    release: UpdateRelease,
    staged: StagedUpdate,
}

#[derive(Clone)]
pub struct UpdateService {
    store: UpdateStateStore,
    adapter: Arc<dyn SignedUpdateAdapter>,
    pending: Arc<Mutex<Option<PendingUpdate>>>,
}

const AUTOMATIC_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone)]
pub struct UpdateManager {
    current_version: String,
    store: UpdateStateStore,
    checker: Arc<dyn UpdateChecker>,
    download_service: UpdateService,
    lifecycle: Arc<Mutex<UpdateLifecycle>>,
    fallback_reason: Arc<Mutex<Option<String>>>,
    staged: Arc<Mutex<Option<StagedUpdate>>>,
}

impl UpdateManager {
    pub fn new(
        current_version: impl Into<String>,
        data_root: &Path,
        app: AppHandle,
    ) -> Result<Self, UpdateManagerError> {
        let policy = default_source_policy().map_err(UpdateServiceError::from)?;
        let transport = HttpManifestTransport::new()?;
        let checker = Arc::new(DualSourceChecker::new(policy.clone(), transport));
        let adapter = Arc::new(TauriSignedUpdateAdapter::new(app, policy));
        let store = UpdateStateStore::new(data_root)?;
        let service = UpdateService::new(data_root, adapter)?;
        Self::new_with_components(current_version, store, checker, service)
    }

    pub fn new_with_components(
        current_version: impl Into<String>,
        store: UpdateStateStore,
        checker: Arc<dyn UpdateChecker>,
        download_service: UpdateService,
    ) -> Result<Self, UpdateManagerError> {
        store.cleanup_partial_files()?;
        Ok(Self {
            current_version: current_version.into(),
            store,
            checker,
            download_service,
            lifecycle: Arc::new(Mutex::new(UpdateLifecycle::default())),
            fallback_reason: Arc::new(Mutex::new(None)),
            staged: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn check(&self, manual: bool, now: u64) -> Result<UpdateStatus, UpdateManagerError> {
        let persistent = self.store.load()?;
        if !manual && !persistent.automatic_check_due(now, AUTOMATIC_CHECK_INTERVAL) {
            return self.status().await;
        }

        self.lifecycle.lock().await.begin_check()?;
        *self.fallback_reason.lock().await = None;

        match self.checker.check(&self.current_version).await {
            Ok(selection) => {
                *self.fallback_reason.lock().await = selection.fallback_reason.clone();
                let result = match selection.decision {
                    ManifestDecision::UpToDate => {
                        self.lifecycle.lock().await.set_up_to_date()?;
                        StoredCheckResult {
                            status: StoredCheckStatus::UpToDate,
                            version: Some(self.current_version.clone()),
                            source: Some(selection.source),
                            message: Some("当前已是最新版本".into()),
                        }
                    }
                    ManifestDecision::Available(release) => {
                        let version = release.version.clone();
                        self.lifecycle.lock().await.set_available(*release)?;
                        StoredCheckResult {
                            status: StoredCheckStatus::Available,
                            version: Some(version),
                            source: Some(selection.source),
                            message: Some("发现可用更新".into()),
                        }
                    }
                };
                self.persist_check(now, result)?;
                self.status().await
            }
            Err(error) => {
                let public_message = public_source_error(&error).to_string();
                self.lifecycle.lock().await.fail(public_message.clone());
                self.persist_check(
                    now,
                    StoredCheckResult {
                        status: StoredCheckStatus::Failed,
                        version: None,
                        source: None,
                        message: Some(public_message),
                    },
                )?;
                Err(error.into())
            }
        }
    }

    pub async fn download(
        &self,
        mut event_callback: Box<dyn FnMut(UpdateProgressEvent) + Send>,
    ) -> Result<UpdateStatus, UpdateManagerError> {
        let release = {
            let mut lifecycle = self.lifecycle.lock().await;
            lifecycle.begin_download()?;
            lifecycle
                .release()
                .cloned()
                .expect("available lifecycle always has release metadata")
        };
        let mut sequence = 0_u64;
        let mut last_downloaded = 0_u64;
        let progress: ProgressCallback = Box::new(move |downloaded, total| {
            sequence = sequence.saturating_add(1);
            last_downloaded = last_downloaded.max(downloaded);
            event_callback(UpdateProgressEvent {
                sequence,
                downloaded_bytes: last_downloaded,
                total_bytes: total,
            });
        });
        match self.download_service.download(release, progress).await {
            Ok(staged) => {
                *self.staged.lock().await = Some(staged);
                self.lifecycle.lock().await.set_downloaded()?;
                self.status().await
            }
            Err(error) => {
                self.lifecycle
                    .lock()
                    .await
                    .fail(public_service_error(&error));
                Err(error.into())
            }
        }
    }

    pub async fn install(&self, confirmed: bool) -> Result<UpdateStatus, UpdateManagerError> {
        if !confirmed {
            return Err(UpdateServiceError::ConfirmationRequired.into());
        }
        self.lifecycle.lock().await.begin_install()?;
        if let Err(error) = self.download_service.install(true).await {
            self.lifecycle
                .lock()
                .await
                .fail(public_service_error(&error));
            return Err(error.into());
        }
        self.status().await
    }

    pub async fn clear_downloaded(&self) -> Result<UpdateStatus, UpdateManagerError> {
        self.download_service.clear_downloaded().await?;
        *self.staged.lock().await = None;
        *self.fallback_reason.lock().await = None;
        self.lifecycle.lock().await.reset();
        self.status().await
    }

    pub async fn set_auto_check(&self, enabled: bool) -> Result<UpdateStatus, UpdateManagerError> {
        let mut state = self.store.load()?;
        state.auto_check = enabled;
        self.store.save(&state)?;
        self.status().await
    }

    pub async fn status(&self) -> Result<UpdateStatus, UpdateManagerError> {
        let persistent = self.store.load()?;
        let lifecycle = self.lifecycle.lock().await;
        Ok(UpdateStatus {
            current_version: self.current_version.clone(),
            phase: lifecycle.phase(),
            auto_check: persistent.auto_check,
            last_checked_at: persistent.last_checked_at,
            last_result: persistent.last_result,
            release: lifecycle.release().map(AvailableUpdate::from),
            fallback_reason: self.fallback_reason.lock().await.clone(),
            staged: self.staged.lock().await.clone(),
            last_error: lifecycle.last_error().map(str::to_owned),
        })
    }

    fn persist_check(&self, now: u64, result: StoredCheckResult) -> Result<(), UpdateManagerError> {
        let mut state = self.store.load()?;
        state.last_checked_at = Some(now);
        state.last_result = Some(result);
        self.store.save(&state)?;
        Ok(())
    }
}

impl UpdateService {
    pub fn new(
        data_root: &Path,
        adapter: Arc<dyn SignedUpdateAdapter>,
    ) -> Result<Self, UpdateServiceError> {
        let store = UpdateStateStore::new(data_root)?;
        store.cleanup_partial_files()?;
        Ok(Self {
            store,
            adapter,
            pending: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn download(
        &self,
        release: UpdateRelease,
        progress: ProgressCallback,
    ) -> Result<StagedUpdate, UpdateServiceError> {
        let bytes = self.adapter.download(&release, progress).await?;
        verify_bytes(&release, &bytes)?;

        let relative_path = staged_relative_path(&release)?;
        let final_path = self.store.resolve_staged_file(&relative_path)?;
        let partial_path = partial_path(&final_path);
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let result = write_verified_file(&partial_path, &final_path, &bytes);
        if result.is_err() {
            let _ = fs::remove_file(&partial_path);
        }
        result?;

        let staged = StagedUpdate {
            version: release.version.clone(),
            relative_path,
            sha256: release.sha256.as_str().into(),
            size: release.size,
        };
        let mut state = self.store.load()?;
        state.staged_file = Some(staged.relative_path.clone());
        self.store.save(&state)?;
        *self.pending.lock().await = Some(PendingUpdate {
            release,
            staged: staged.clone(),
        });
        Ok(staged)
    }

    pub async fn install(&self, confirmed: bool) -> Result<(), UpdateServiceError> {
        if !confirmed {
            return Err(UpdateServiceError::ConfirmationRequired);
        }
        let pending = self
            .pending
            .lock()
            .await
            .clone()
            .ok_or(UpdateServiceError::NotDownloaded)?;
        let path = self
            .store
            .resolve_staged_file(&pending.staged.relative_path)?;
        let bytes = fs::read(path)?;
        verify_bytes(&pending.release, &bytes)?;
        self.adapter
            .install(&pending.release, bytes)
            .await
            .map_err(Into::into)
    }

    pub async fn clear_downloaded(&self) -> Result<bool, UpdateServiceError> {
        let pending = self.pending.lock().await.take();
        let mut removed = false;
        let mut state = self.store.load()?;
        let relative_path = pending
            .as_ref()
            .map(|pending| pending.staged.relative_path.as_str())
            .or(state.staged_file.as_deref());
        if let Some(relative_path) = relative_path {
            let path = self.store.resolve_staged_file(relative_path)?;
            match fs::remove_file(path) {
                Ok(()) => removed = true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        state.staged_file = None;
        self.store.save(&state)?;
        Ok(removed)
    }
}

fn staged_relative_path(release: &UpdateRelease) -> Result<String, UpdateServiceError> {
    let suffix = match release.platform.as_str() {
        "windows-x86_64" | "windows-x86_64-nsis" | "windows-aarch64-nsis" => "exe",
        "macos-x86_64-dmg" | "macos-aarch64-dmg" => "app.tar.gz",
        "linux-x86_64-appimage" | "linux-aarch64-appimage" => "AppImage",
        _ => return Err(UpdateServiceError::Integrity),
    };
    Ok(format!(
        "staged/{}/QingzhouSSH-{}-{}.{}",
        release.version, release.version, release.platform, suffix
    ))
}

fn verify_bytes(release: &UpdateRelease, bytes: &[u8]) -> Result<(), UpdateServiceError> {
    if bytes.len() as u64 != release.size {
        return Err(UpdateServiceError::Integrity);
    }
    let digest = format!("{:x}", Sha256::digest(bytes));
    if digest != release.sha256.as_str() {
        return Err(UpdateServiceError::Integrity);
    }
    Ok(())
}

fn partial_path(final_path: &Path) -> PathBuf {
    let mut name = final_path
        .file_name()
        .expect("staged update always has a file name")
        .to_os_string();
    name.push(".partial");
    final_path.with_file_name(name)
}

fn write_verified_file(
    partial_path: &Path,
    final_path: &Path,
    bytes: &[u8],
) -> std::io::Result<()> {
    if final_path.exists() {
        let existing = fs::read(final_path)?;
        if existing == bytes {
            return Ok(());
        }
        fs::remove_file(final_path)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(partial_path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(partial_path, final_path)
}

#[derive(Clone)]
pub struct TauriSignedUpdateAdapter {
    app: AppHandle,
    policy: TrustedSourcePolicy,
}

impl TauriSignedUpdateAdapter {
    pub fn new(app: AppHandle, policy: TrustedSourcePolicy) -> Self {
        Self { app, policy }
    }

    async fn resolve_update(&self, expected: &UpdateRelease) -> Result<Update, UpdateAdapterError> {
        let endpoint = self.policy.manifest_endpoint(expected.source);
        let endpoint = Url::parse(&endpoint).map_err(|_| UpdateAdapterError::Manifest)?;
        let updater = self
            .app
            .updater_builder()
            .target(expected.platform.clone())
            .endpoints(vec![endpoint])
            .map_err(map_updater_error)?
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(map_updater_error)?;
        let update = updater
            .check()
            .await
            .map_err(map_updater_error)?
            .ok_or(UpdateAdapterError::Manifest)?;
        let raw = serde_json::to_vec(&update.raw_json).map_err(|_| UpdateAdapterError::Manifest)?;
        let decision = parse_manifest(
            &self.policy,
            expected.source,
            &self.app.package_info().version.to_string(),
            &raw,
        )
        .map_err(|_| UpdateAdapterError::Manifest)?;
        let ManifestDecision::Available(actual) = decision else {
            return Err(UpdateAdapterError::Manifest);
        };
        if actual.as_ref() != expected
            || update.version != expected.version
            || update.download_url.as_str() != expected.download_url
            || update.signature != expected.signature
        {
            return Err(UpdateAdapterError::Manifest);
        }
        Ok(update)
    }
}

impl SignedUpdateAdapter for TauriSignedUpdateAdapter {
    fn download<'a>(
        &'a self,
        release: &'a UpdateRelease,
        mut progress: ProgressCallback,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, UpdateAdapterError>> + Send + 'a>> {
        Box::pin(async move {
            let update = self.resolve_update(release).await?;
            let mut downloaded = 0_u64;
            update
                .download(
                    |chunk, total| {
                        downloaded = downloaded.saturating_add(chunk as u64);
                        progress(downloaded, total);
                    },
                    || {},
                )
                .await
                .map_err(map_updater_error)
        })
    }

    fn install<'a>(
        &'a self,
        release: &'a UpdateRelease,
        bytes: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), UpdateAdapterError>> + Send + 'a>> {
        Box::pin(async move {
            let update = self.resolve_update(release).await?;
            update
                .install(bytes)
                .map_err(|_| UpdateAdapterError::Install)
        })
    }
}

fn map_updater_error(error: tauri_plugin_updater::Error) -> UpdateAdapterError {
    match error {
        tauri_plugin_updater::Error::Minisign(_)
        | tauri_plugin_updater::Error::Base64(_)
        | tauri_plugin_updater::Error::SignatureUtf8(_) => UpdateAdapterError::Signature,
        tauri_plugin_updater::Error::Network(_)
        | tauri_plugin_updater::Error::Reqwest(_)
        | tauri_plugin_updater::Error::Http(_) => UpdateAdapterError::Network,
        _ => UpdateAdapterError::Manifest,
    }
}

fn public_source_error(error: &SourceCheckError) -> &'static str {
    match error.kind {
        SourceFailureKind::Network => "更新网络暂时不可用",
        SourceFailureKind::NotFound => "更新源尚未发布版本清单",
        SourceFailureKind::Server => "更新服务暂时不可用",
        SourceFailureKind::InvalidManifest => "更新清单格式或内容无效",
        SourceFailureKind::Security => "更新源安全校验失败",
    }
}

fn public_service_error(error: &UpdateServiceError) -> String {
    match error {
        UpdateServiceError::Adapter(UpdateAdapterError::Network) => "更新网络暂时不可用".into(),
        UpdateServiceError::Adapter(UpdateAdapterError::Signature) => "更新签名验证失败".into(),
        UpdateServiceError::Adapter(UpdateAdapterError::Manifest) => "更新清单校验失败".into(),
        UpdateServiceError::Adapter(UpdateAdapterError::Install) => "更新安装器启动失败".into(),
        UpdateServiceError::Integrity => "更新包完整性校验失败".into(),
        UpdateServiceError::NotDownloaded => "更新包尚未完成下载".into(),
        UpdateServiceError::ConfirmationRequired => "安装更新前必须明确确认".into(),
        UpdateServiceError::Store(_) => "更新状态存储失败".into(),
        UpdateServiceError::Io(_) => "更新文件操作失败".into(),
    }
}

pub fn default_source_policy() -> Result<TrustedSourcePolicy, UpdateAdapterError> {
    TrustedSourcePolicy::new(
        "lx3559359",
        option_env!("QINGZHOU_MODELSCOPE_NAMESPACE").unwrap_or("lx3559359"),
    )
    .map_err(|_| UpdateAdapterError::Manifest)
}

pub fn source_label(source: UpdateSource) -> &'static str {
    match source {
        UpdateSource::Github => "GitHub Releases",
        UpdateSource::Modelscope => "ModelScope 国内镜像",
    }
}
