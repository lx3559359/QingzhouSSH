use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskCategory {
    System,
    Storage,
    Network,
    Security,
    Service,
    Logs,
    Web,
    Container,
    Script,
    Advanced,
}

impl TaskCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Storage => "storage",
            Self::Network => "network",
            Self::Security => "security",
            Self::Service => "service",
            Self::Logs => "logs",
            Self::Web => "web",
            Self::Container => "container",
            Self::Script => "script",
            Self::Advanced => "advanced",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegeRequirement {
    CurrentUser,
    RootOrPasswordlessSudo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionScope {
    SingleServer,
    ReadOnlyBatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Safe,
    Caution,
    Dangerous,
}

impl RiskLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Caution => "caution",
            Self::Dangerous => "dangerous",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ParameterKind {
    String {
        min_length: usize,
        max_length: usize,
        multiline: bool,
    },
    Integer {
        min: i64,
        max: i64,
    },
    Boolean,
    Enum {
        options: Vec<String>,
    },
    AbsolutePath,
    ServiceName,
    TimeRange,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterDefinition {
    pub name: String,
    pub label: String,
    pub description: String,
    pub kind: ParameterKind,
    pub required: bool,
    pub default_value: Option<Value>,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibilityPredicate {
    pub os_families: Vec<String>,
    pub service_managers: Vec<String>,
    pub required_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStep {
    pub id: String,
    pub title: String,
    pub timeout_seconds: u64,
    pub output_limit_bytes: u64,
    #[serde(skip_serializing, skip_deserializing)]
    pub command_template: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupItemKind {
    RemoteFile,
    CommandSnapshot,
    ManagedBlock,
    RuntimeState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupItemDefinition {
    pub id: String,
    pub kind: BackupItemKind,
    #[serde(skip_serializing, skip_deserializing)]
    pub target_template: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupPlan {
    pub items: Vec<BackupItemDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackPlan {
    pub steps: Vec<TaskStep>,
    pub automatic_on_failure: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultParserKind {
    Text,
    KeyValue,
    Table,
    HealthSummary,
    NetworkProbe,
    ServiceStatus,
    ContainerStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskImplementation {
    pub id: String,
    pub compatibility: CompatibilityPredicate,
    pub preflight_steps: Vec<TaskStep>,
    pub preview_steps: Vec<TaskStep>,
    pub backup_plan: Option<BackupPlan>,
    pub execution_steps: Vec<TaskStep>,
    pub verify_steps: Vec<TaskStep>,
    pub rollback_plan: Option<RollbackPlan>,
    pub result_parser: ResultParserKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDefinition {
    pub id: String,
    pub version: i32,
    pub category: TaskCategory,
    pub title: String,
    pub description: String,
    pub risk_level: RiskLevel,
    pub estimated_seconds: u32,
    pub privilege: PrivilegeRequirement,
    pub scope: ExecutionScope,
    pub parameters: Vec<ParameterDefinition>,
    pub implementations: Vec<TaskImplementation>,
    pub output_kind: OutputKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    Text,
    Table,
    KeyValue,
    LogMatches,
}
