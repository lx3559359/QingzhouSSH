use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_UPDATE_BYTES: u64 = 512 * 1024 * 1024;
pub const SUPPORTED_UPDATE_PLATFORMS: &[&str] = &[
    "windows-x86_64-nsis",
    "windows-aarch64-nsis",
    "macos-x86_64-dmg",
    "macos-aarch64-dmg",
    "linux-x86_64-appimage",
    "linux-aarch64-appimage",
    // Read-only compatibility for manifests published before the platform matrix.
    "windows-x86_64",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateSource {
    Github,
    Modelscope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Downloaded,
    Installing,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UpdateValidationError {
    #[error("更新版本不是有效的 SemVer")]
    InvalidVersion,
    #[error("更新平台不受支持")]
    InvalidPlatform,
    #[error("更新签名缺失或过长")]
    InvalidSignature,
    #[error("更新 SHA-256 格式无效")]
    InvalidSha256,
    #[error("更新包大小无效")]
    InvalidSize,
    #[error("更新构建标识无效")]
    InvalidBuildId,
    #[error("更新下载地址为空")]
    InvalidUrl,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("更新状态不能从 {from:?} 转换为 {to:?}")]
pub struct UpdateTransitionError {
    pub from: UpdatePhase,
    pub to: UpdatePhase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    pub fn parse(value: impl Into<String>) -> Result<Self, UpdateValidationError> {
        let value = value.into();
        let valid = value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        valid
            .then_some(Self(value))
            .ok_or(UpdateValidationError::InvalidSha256)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct UpdateReleaseInput {
    pub version: String,
    pub notes: String,
    pub published_at: Option<String>,
    pub platform: String,
    pub download_url: String,
    pub signature: String,
    pub sha256: String,
    pub size: u64,
    pub build_id: String,
    pub source: UpdateSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateRelease {
    pub version: String,
    pub notes: String,
    pub published_at: Option<String>,
    pub platform: String,
    pub download_url: String,
    pub signature: String,
    pub sha256: Sha256Digest,
    pub size: u64,
    pub build_id: String,
    pub source: UpdateSource,
}

impl UpdateRelease {
    pub fn new(input: UpdateReleaseInput) -> Result<Self, UpdateValidationError> {
        Version::parse(&input.version).map_err(|_| UpdateValidationError::InvalidVersion)?;
        if !SUPPORTED_UPDATE_PLATFORMS.contains(&input.platform.as_str()) {
            return Err(UpdateValidationError::InvalidPlatform);
        }
        if input.download_url.trim().is_empty() {
            return Err(UpdateValidationError::InvalidUrl);
        }
        if input.signature.trim().is_empty() || input.signature.len() > 16 * 1024 {
            return Err(UpdateValidationError::InvalidSignature);
        }
        if input.size == 0 || input.size > MAX_UPDATE_BYTES {
            return Err(UpdateValidationError::InvalidSize);
        }
        if input.build_id.trim().is_empty() || input.build_id.len() > 128 {
            return Err(UpdateValidationError::InvalidBuildId);
        }
        Ok(Self {
            version: input.version,
            notes: input.notes,
            published_at: input.published_at,
            platform: input.platform,
            download_url: input.download_url,
            signature: input.signature,
            sha256: Sha256Digest::parse(input.sha256)?,
            size: input.size,
            build_id: input.build_id,
            source: input.source,
        })
    }

    pub fn is_newer_than(&self, current: &str) -> Result<bool, UpdateValidationError> {
        let candidate =
            Version::parse(&self.version).map_err(|_| UpdateValidationError::InvalidVersion)?;
        let current = Version::parse(current).map_err(|_| UpdateValidationError::InvalidVersion)?;
        Ok(candidate > current)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLifecycle {
    phase: UpdatePhase,
    release: Option<UpdateRelease>,
    last_error: Option<String>,
}

impl Default for UpdateLifecycle {
    fn default() -> Self {
        Self {
            phase: UpdatePhase::Idle,
            release: None,
            last_error: None,
        }
    }
}

impl UpdateLifecycle {
    pub fn phase(&self) -> UpdatePhase {
        self.phase
    }

    pub fn release(&self) -> Option<&UpdateRelease> {
        self.release.as_ref()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn begin_check(&mut self) -> Result<(), UpdateTransitionError> {
        match self.phase {
            UpdatePhase::Idle
            | UpdatePhase::UpToDate
            | UpdatePhase::Available
            | UpdatePhase::Failed => {
                self.phase = UpdatePhase::Checking;
                self.release = None;
                self.last_error = None;
                Ok(())
            }
            from => Err(transition(from, UpdatePhase::Checking)),
        }
    }

    pub fn set_up_to_date(&mut self) -> Result<(), UpdateTransitionError> {
        self.require(UpdatePhase::Checking, UpdatePhase::UpToDate)?;
        self.phase = UpdatePhase::UpToDate;
        self.release = None;
        Ok(())
    }

    pub fn set_available(&mut self, release: UpdateRelease) -> Result<(), UpdateTransitionError> {
        self.require(UpdatePhase::Checking, UpdatePhase::Available)?;
        self.phase = UpdatePhase::Available;
        self.release = Some(release);
        Ok(())
    }

    pub fn begin_download(&mut self) -> Result<(), UpdateTransitionError> {
        self.require(UpdatePhase::Available, UpdatePhase::Downloading)?;
        self.phase = UpdatePhase::Downloading;
        Ok(())
    }

    pub fn set_downloaded(&mut self) -> Result<(), UpdateTransitionError> {
        self.require(UpdatePhase::Downloading, UpdatePhase::Downloaded)?;
        self.phase = UpdatePhase::Downloaded;
        Ok(())
    }

    pub fn begin_install(&mut self) -> Result<(), UpdateTransitionError> {
        self.require(UpdatePhase::Downloaded, UpdatePhase::Installing)?;
        self.phase = UpdatePhase::Installing;
        Ok(())
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        self.phase = UpdatePhase::Failed;
        self.last_error = Some(message.into());
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    fn require(
        &self,
        expected: UpdatePhase,
        next: UpdatePhase,
    ) -> Result<(), UpdateTransitionError> {
        (self.phase == expected)
            .then_some(())
            .ok_or_else(|| transition(self.phase, next))
    }
}

fn transition(from: UpdatePhase, to: UpdatePhase) -> UpdateTransitionError {
    UpdateTransitionError { from, to }
}
