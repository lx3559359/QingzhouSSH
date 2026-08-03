use std::{collections::BTreeMap, future::Future, pin::Pin};

use serde::Deserialize;
use thiserror::Error;
use url::Url;

use crate::domain::update::{
    UpdateRelease, UpdateReleaseInput, UpdateSource, UpdateValidationError,
};

mod state_store;
pub use state_store::{
    StoredCheckResult, StoredCheckStatus, UpdatePersistentState, UpdateStateStore,
    UpdateStateStoreError,
};

const PROJECT_NAME: &str = "QingzhouSSH";
const PLATFORM: &str = "windows-x86_64";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedSourcePolicy {
    github_owner: String,
    modelscope_namespace: String,
}

impl TrustedSourcePolicy {
    pub fn new(
        github_owner: impl Into<String>,
        modelscope_namespace: impl Into<String>,
    ) -> Result<Self, SourceCheckError> {
        let github_owner = github_owner.into();
        let modelscope_namespace = modelscope_namespace.into();
        if !valid_namespace(&github_owner) || !valid_namespace(&modelscope_namespace) {
            return Err(SourceCheckError::new(
                SourceFailureKind::Security,
                "更新源命名空间无效",
            ));
        }
        Ok(Self {
            github_owner,
            modelscope_namespace,
        })
    }

    pub fn manifest_endpoint(&self, source: UpdateSource) -> String {
        match source {
            UpdateSource::Github => format!(
                "https://github.com/{}/{PROJECT_NAME}/releases/latest/download/latest.json",
                self.github_owner
            ),
            UpdateSource::Modelscope => format!(
                "https://modelscope.cn/api/v1/studios/{}/{PROJECT_NAME}/repo?Revision=master&FilePath=releases%2Flatest.json",
                self.modelscope_namespace
            ),
        }
    }

    fn validate_download_url(
        &self,
        source: UpdateSource,
        value: &str,
    ) -> Result<(), SourceCheckError> {
        let url = Url::parse(value).map_err(|_| security("更新包地址无效"))?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.fragment().is_some()
        {
            return Err(security("更新包必须使用无凭据的标准 HTTPS 地址"));
        }
        match source {
            UpdateSource::Github => self.validate_github_url(&url),
            UpdateSource::Modelscope => self.validate_modelscope_url(&url),
        }
    }

    fn validate_github_url(&self, url: &Url) -> Result<(), SourceCheckError> {
        let expected_prefix = format!("/{}/{PROJECT_NAME}/releases/download/", self.github_owner);
        if url.host_str() != Some("github.com")
            || !url.path().starts_with(&expected_prefix)
            || url.query().is_some()
        {
            return Err(security("GitHub 更新包不属于固定公开仓库"));
        }
        Ok(())
    }

    fn validate_modelscope_url(&self, url: &Url) -> Result<(), SourceCheckError> {
        let host_ok = matches!(url.host_str(), Some("modelscope.cn" | "www.modelscope.cn"));
        let expected_path = format!(
            "/api/v1/studios/{}/{PROJECT_NAME}/repo",
            self.modelscope_namespace
        );
        if !host_ok || url.path() != expected_path {
            return Err(security("ModelScope 更新包不属于固定公开项目"));
        }
        let mut revision = None;
        let mut file_path = None;
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "Revision" if revision.is_none() => revision = Some(value.into_owned()),
                "FilePath" if file_path.is_none() => file_path = Some(value.into_owned()),
                _ => return Err(security("ModelScope 更新地址包含未知参数")),
            }
        }
        let file_path = file_path.ok_or_else(|| security("ModelScope 更新地址缺少文件路径"))?;
        if revision.as_deref() != Some("master")
            || !file_path.starts_with("releases/")
            || file_path.contains("..")
            || file_path.contains('\\')
            || file_path.starts_with('/')
        {
            return Err(security("ModelScope 更新文件越过 releases 目录"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFailureKind {
    Network,
    NotFound,
    Server,
    InvalidManifest,
    Security,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct SourceCheckError {
    pub kind: SourceFailureKind,
    pub message: String,
}

impl SourceCheckError {
    pub fn new(kind: SourceFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn allows_fallback(&self) -> bool {
        matches!(
            self.kind,
            SourceFailureKind::Network | SourceFailureKind::NotFound | SourceFailureKind::Server
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestDecision {
    UpToDate,
    Available(Box<UpdateRelease>),
}

pub trait ManifestTransport: Send + Sync {
    fn fetch<'a>(
        &'a self,
        endpoint: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, SourceCheckError>> + Send + 'a>>;
}

pub trait UpdateChecker: Send + Sync {
    fn check<'a>(
        &'a self,
        current_version: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<SourceSelection, SourceCheckError>> + Send + 'a>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSelection {
    pub source: UpdateSource,
    pub decision: ManifestDecision,
    pub fallback_reason: Option<String>,
}

pub struct DualSourceChecker<T> {
    policy: TrustedSourcePolicy,
    transport: T,
}

impl<T: ManifestTransport> DualSourceChecker<T> {
    pub fn new(policy: TrustedSourcePolicy, transport: T) -> Self {
        Self { policy, transport }
    }

    pub async fn check(&self, current_version: &str) -> Result<SourceSelection, SourceCheckError> {
        let github = self
            .check_source(UpdateSource::Github, current_version)
            .await;
        match github {
            Ok(decision) => Ok(SourceSelection {
                source: UpdateSource::Github,
                decision,
                fallback_reason: None,
            }),
            Err(error) if error.allows_fallback() => {
                let reason = error.message;
                let decision = self
                    .check_source(UpdateSource::Modelscope, current_version)
                    .await?;
                Ok(SourceSelection {
                    source: UpdateSource::Modelscope,
                    decision,
                    fallback_reason: Some(reason),
                })
            }
            Err(error) => Err(error),
        }
    }

    async fn check_source(
        &self,
        source: UpdateSource,
        current_version: &str,
    ) -> Result<ManifestDecision, SourceCheckError> {
        let endpoint = self.policy.manifest_endpoint(source);
        let bytes = self.transport.fetch(&endpoint).await?;
        parse_manifest(&self.policy, source, current_version, &bytes)
    }
}

impl<T: ManifestTransport> UpdateChecker for DualSourceChecker<T> {
    fn check<'a>(
        &'a self,
        current_version: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<SourceSelection, SourceCheckError>> + Send + 'a>> {
        Box::pin(async move { DualSourceChecker::check(self, current_version).await })
    }
}

#[derive(Clone)]
pub struct HttpManifestTransport {
    client: reqwest::Client,
}

impl HttpManifestTransport {
    pub fn new() -> Result<Self, SourceCheckError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|_| {
                SourceCheckError::new(SourceFailureKind::Network, "无法初始化更新网络客户端")
            })?;
        Ok(Self { client })
    }
}

impl ManifestTransport for HttpManifestTransport {
    fn fetch<'a>(
        &'a self,
        endpoint: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, SourceCheckError>> + Send + 'a>> {
        Box::pin(async move {
            const MAX_MANIFEST_BYTES: u64 = 128 * 1024;
            let response = self
                .client
                .get(endpoint)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await
                .map_err(|_| {
                    SourceCheckError::new(SourceFailureKind::Network, "更新源网络请求失败")
                })?;
            let status = response.status();
            if status == reqwest::StatusCode::NOT_FOUND {
                return Err(SourceCheckError::new(
                    SourceFailureKind::NotFound,
                    "更新源尚未发布清单",
                ));
            }
            if status.is_server_error() {
                return Err(SourceCheckError::new(
                    SourceFailureKind::Server,
                    "更新源服务暂时不可用",
                ));
            }
            if !status.is_success() {
                return Err(SourceCheckError::new(
                    SourceFailureKind::Security,
                    "更新源拒绝访问",
                ));
            }
            if response
                .content_length()
                .is_some_and(|length| length > MAX_MANIFEST_BYTES)
            {
                return Err(invalid_manifest("更新清单超过大小上限"));
            }
            let bytes = response.bytes().await.map_err(|_| {
                SourceCheckError::new(SourceFailureKind::Network, "读取更新清单失败")
            })?;
            if bytes.len() as u64 > MAX_MANIFEST_BYTES {
                return Err(invalid_manifest("更新清单超过大小上限"));
            }
            Ok(bytes.to_vec())
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticManifest {
    version: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    pub_date: Option<String>,
    platforms: BTreeMap<String, PlatformManifest>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlatformManifest {
    url: String,
    signature: String,
    sha256: String,
    size: u64,
    build_id: String,
}

pub fn parse_manifest(
    policy: &TrustedSourcePolicy,
    source: UpdateSource,
    current_version: &str,
    bytes: &[u8],
) -> Result<ManifestDecision, SourceCheckError> {
    const MAX_MANIFEST_BYTES: usize = 128 * 1024;
    if bytes.is_empty() || bytes.len() > MAX_MANIFEST_BYTES {
        return Err(invalid_manifest("更新清单大小无效"));
    }
    let manifest: StaticManifest =
        serde_json::from_slice(bytes).map_err(|_| invalid_manifest("更新清单格式无效"))?;
    let platform = manifest
        .platforms
        .get(PLATFORM)
        .ok_or_else(|| invalid_manifest("更新清单缺少 Windows x64 平台"))?;
    policy.validate_download_url(source, &platform.url)?;
    let release = UpdateRelease::new(UpdateReleaseInput {
        version: manifest.version,
        notes: manifest.notes,
        published_at: manifest.pub_date,
        platform: PLATFORM.into(),
        download_url: platform.url.clone(),
        signature: platform.signature.clone(),
        sha256: platform.sha256.clone(),
        size: platform.size,
        build_id: platform.build_id.clone(),
        source,
    })
    .map_err(map_validation_error)?;
    if release
        .is_newer_than(current_version)
        .map_err(map_validation_error)?
    {
        Ok(ManifestDecision::Available(Box::new(release)))
    } else {
        Ok(ManifestDecision::UpToDate)
    }
}

pub fn choose_source<T, F>(
    primary: Result<T, SourceCheckError>,
    fallback: F,
) -> Result<T, SourceCheckError>
where
    F: FnOnce() -> Result<T, SourceCheckError>,
{
    match primary {
        Ok(value) => Ok(value),
        Err(error) if error.allows_fallback() => fallback(),
        Err(error) => Err(error),
    }
}

fn valid_namespace(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn map_validation_error(error: UpdateValidationError) -> SourceCheckError {
    invalid_manifest(error.to_string())
}

fn invalid_manifest(message: impl Into<String>) -> SourceCheckError {
    SourceCheckError::new(SourceFailureKind::InvalidManifest, message)
}

fn security(message: impl Into<String>) -> SourceCheckError {
    SourceCheckError::new(SourceFailureKind::Security, message)
}
