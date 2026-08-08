use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    core::system_probe::SystemCapabilities,
    error::{AppError, AppResult},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptShell {
    #[default]
    PosixSh,
    Bash,
    PowerShell,
}

impl ScriptShell {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PosixSh => "posix_sh",
            Self::Bash => "bash",
            Self::PowerShell => "powershell",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::PosixSh => "POSIX sh",
            Self::Bash => "Bash",
            Self::PowerShell => "PowerShell",
        }
    }

    pub fn ensure_supported(self, capabilities: &SystemCapabilities) -> AppResult<()> {
        self.executable(capabilities).map(|_| ())
    }

    pub fn executable(self, capabilities: &SystemCapabilities) -> AppResult<&'static str> {
        match self {
            Self::PosixSh if capabilities.has_command("sh") => Ok("sh"),
            Self::Bash if capabilities.has_command("bash") => Ok("bash"),
            Self::PowerShell if capabilities.has_command("pwsh") => Ok("pwsh"),
            Self::PowerShell if capabilities.has_command("powershell") => Ok("powershell"),
            _ => Err(AppError::Compatibility(format!(
                "目标服务器未探测到 {}",
                self.display_name()
            ))),
        }
    }
}

impl TryFrom<&str> for ScriptShell {
    type Error = AppError;

    fn try_from(value: &str) -> AppResult<Self> {
        match value {
            "posix_sh" => Ok(Self::PosixSh),
            "bash" => Ok(Self::Bash),
            "powershell" => Ok(Self::PowerShell),
            other => Err(AppError::Validation(format!("未知脚本 Shell：{other}"))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptCompatibility {
    pub os_families: Vec<String>,
    pub required_commands: Vec<String>,
}

impl ScriptCompatibility {
    pub fn for_shell(shell: ScriptShell) -> Self {
        match shell {
            ScriptShell::PosixSh => Self {
                os_families: vec!["linux".into(), "bsd".into()],
                required_commands: vec!["sh".into()],
            },
            ScriptShell::Bash => Self {
                os_families: vec!["linux".into(), "bsd".into()],
                required_commands: vec!["bash".into()],
            },
            ScriptShell::PowerShell => Self {
                os_families: vec!["windows".into(), "linux".into(), "macos".into()],
                required_commands: vec!["powershell_or_pwsh".into()],
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptDefinition {
    pub id: Uuid,
    pub title: String,
    pub category: String,
    pub tags: Vec<String>,
    pub is_favorite: bool,
    pub is_enabled: bool,
    pub active_version_id: Uuid,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptVersion {
    pub id: Uuid,
    pub definition_id: Uuid,
    pub version_number: u32,
    pub body: String,
    pub body_sha256: String,
    pub parameters: Value,
    pub scan_summary: Value,
    pub timeout_seconds: u64,
    pub shell: ScriptShell,
    pub compatibility: ScriptCompatibility,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptDetails {
    pub definition: ScriptDefinition,
    pub active_version: ScriptVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptSummary {
    pub id: Uuid,
    pub title: String,
    pub category: String,
    pub tags: Vec<String>,
    pub is_favorite: bool,
    pub is_enabled: bool,
    pub active_version_id: Uuid,
    pub active_version_number: u32,
    pub body_sha256: String,
    pub shell: ScriptShell,
    pub compatibility: ScriptCompatibility,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewScriptVersion {
    pub body: String,
    pub parameters: Value,
    pub scan_summary: Value,
    pub timeout_seconds: u64,
    pub shell: ScriptShell,
    pub compatibility: ScriptCompatibility,
}

#[derive(Debug, Clone)]
pub struct NewPersonalScript {
    pub title: String,
    pub category: String,
    pub tags: Vec<String>,
    pub is_favorite: bool,
    pub is_enabled: bool,
    pub version: NewScriptVersion,
}

#[derive(Debug, Clone)]
pub struct ScriptMetadataUpdate {
    pub title: String,
    pub category: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScriptListFilter {
    pub query: Option<String>,
    pub category: Option<String>,
    pub tag: Option<String>,
    pub favorite: Option<bool>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptRunReference {
    pub id: Uuid,
    pub definition_id: Uuid,
    pub version_id: Uuid,
    pub operation_run_id: Uuid,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewScriptRunReference {
    pub definition_id: Uuid,
    pub version_id: Uuid,
    pub operation_run_id: Uuid,
}
