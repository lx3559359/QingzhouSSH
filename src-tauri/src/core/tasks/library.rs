use serde::{Deserialize, Serialize};

use crate::core::tasks::{RiskLevel, TaskCategory, TaskDefinition};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSource {
    BuiltInTask,
    ReviewedCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolLibraryCategory {
    RecommendedRecent,
    DailyInspection,
    Performance,
    Storage,
    Network,
    WebService,
    SecurityLogin,
    ServiceManagement,
    Container,
    SystemSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolLibraryMetadata {
    pub source: ToolSource,
    pub primary_category: ToolLibraryCategory,
    pub keywords: Vec<String>,
    pub novice_aliases: Vec<String>,
}

pub fn metadata_for(definition: &TaskDefinition) -> ToolLibraryMetadata {
    let (primary_category, novice_aliases): (ToolLibraryCategory, Vec<&str>) =
        match definition.id.as_str() {
            "runbook.web.gateway" => (
                ToolLibraryCategory::WebService,
                vec!["网站打不开", "网页访问失败", "网关异常"],
            ),
            "network.port_process" => (
                ToolLibraryCategory::Network,
                vec!["端口被占用", "谁占用了端口", "端口冲突"],
            ),
            "runbook.storage.capacity_io" => (
                ToolLibraryCategory::Storage,
                vec!["磁盘满了", "磁盘空间不足", "磁盘很慢"],
            ),
            "runbook.cpu.incident" => (
                ToolLibraryCategory::Performance,
                vec!["服务器很慢", "CPU 很高", "系统卡顿"],
            ),
            "security.ssh_events" => (
                ToolLibraryCategory::SecurityLogin,
                vec!["登录失败", "SSH 登录异常", "有人尝试登录"],
            ),
            _ => (
                fallback_category(definition.category),
                fallback_aliases(definition.category),
            ),
        };

    let source = if definition.risk_level == RiskLevel::Safe
        && definition
            .implementations
            .iter()
            .all(|implementation| implementation.execution_steps.len() == 1)
        && !definition.id.starts_with("runbook.")
    {
        ToolSource::ReviewedCommand
    } else {
        ToolSource::BuiltInTask
    };

    let mut keywords = vec![
        definition.title.clone(),
        definition.description.clone(),
        category_keyword(primary_category).into(),
    ];
    keywords.sort();
    keywords.dedup();

    ToolLibraryMetadata {
        source,
        primary_category,
        keywords,
        novice_aliases: novice_aliases.into_iter().map(str::to_string).collect(),
    }
}

fn fallback_category(category: TaskCategory) -> ToolLibraryCategory {
    match category {
        TaskCategory::System | TaskCategory::Logs => ToolLibraryCategory::DailyInspection,
        TaskCategory::Storage => ToolLibraryCategory::Storage,
        TaskCategory::Network => ToolLibraryCategory::Network,
        TaskCategory::Security => ToolLibraryCategory::SecurityLogin,
        TaskCategory::Service => ToolLibraryCategory::ServiceManagement,
        TaskCategory::Web => ToolLibraryCategory::WebService,
        TaskCategory::Container => ToolLibraryCategory::Container,
        TaskCategory::Script | TaskCategory::Advanced => ToolLibraryCategory::SystemSettings,
    }
}

fn fallback_aliases(category: TaskCategory) -> Vec<&'static str> {
    match category {
        TaskCategory::System => vec!["检查服务器", "查看系统状态"],
        TaskCategory::Storage => vec!["检查磁盘", "查看存储"],
        TaskCategory::Network => vec!["检查网络", "网络不通"],
        TaskCategory::Security => vec!["安全检查", "登录异常"],
        TaskCategory::Service => vec!["服务有问题", "管理服务"],
        TaskCategory::Logs => vec!["查看日志", "查找错误"],
        TaskCategory::Web => vec!["网站有问题", "检查网页服务"],
        TaskCategory::Container => vec!["查看容器", "容器有问题"],
        TaskCategory::Script => vec!["运行脚本", "我的脚本"],
        TaskCategory::Advanced => vec!["系统设置", "高级操作"],
    }
}

fn category_keyword(category: ToolLibraryCategory) -> &'static str {
    match category {
        ToolLibraryCategory::RecommendedRecent => "推荐与最近",
        ToolLibraryCategory::DailyInspection => "日常巡检",
        ToolLibraryCategory::Performance => "性能与卡顿",
        ToolLibraryCategory::Storage => "磁盘与存储",
        ToolLibraryCategory::Network => "网络与端口",
        ToolLibraryCategory::WebService => "网站与应用",
        ToolLibraryCategory::SecurityLogin => "安全与登录",
        ToolLibraryCategory::ServiceManagement => "服务管理",
        ToolLibraryCategory::Container => "容器",
        ToolLibraryCategory::SystemSettings => "系统设置",
    }
}
