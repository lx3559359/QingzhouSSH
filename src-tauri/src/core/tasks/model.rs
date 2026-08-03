use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskCategory {
    System,
    Service,
    Logs,
    Advanced,
}

impl TaskCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Service => "service",
            Self::Logs => "logs",
            Self::Advanced => "advanced",
        }
    }
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
pub struct TaskImplementation {
    pub id: String,
    pub compatibility: CompatibilityPredicate,
    #[serde(skip_serializing)]
    pub command_template: String,
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
