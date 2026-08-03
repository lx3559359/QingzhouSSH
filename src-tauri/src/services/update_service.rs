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
        parse_manifest, ManifestDecision, TrustedSourcePolicy, UpdateStateStore,
        UpdateStateStoreError,
    },
    domain::update::{UpdateRelease, UpdateSource},
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

        let relative_path = format!(
            "staged/{}/QingzhouSSH-{}-windows-x86_64.nsis",
            release.version, release.version
        );
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
        if let Some(pending) = pending {
            let path = self
                .store
                .resolve_staged_file(&pending.staged.relative_path)?;
            match fs::remove_file(path) {
                Ok(()) => removed = true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        let mut state = self.store.load()?;
        state.staged_file = None;
        self.store.save(&state)?;
        Ok(removed)
    }
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

pub fn default_source_policy() -> Result<TrustedSourcePolicy, UpdateAdapterError> {
    TrustedSourcePolicy::new(
        "lx3559359",
        option_env!("QINGZHOU_MODELSCOPE_NAMESPACE").unwrap_or("unconfigured"),
    )
    .map_err(|_| UpdateAdapterError::Manifest)
}

pub fn source_label(source: UpdateSource) -> &'static str {
    match source {
        UpdateSource::Github => "GitHub Releases",
        UpdateSource::Modelscope => "ModelScope 国内镜像",
    }
}
