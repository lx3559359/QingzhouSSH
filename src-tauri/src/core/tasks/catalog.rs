use serde_json::json;

use std::collections::BTreeSet;

use crate::core::tasks::model::{
    CompatibilityPredicate, ExecutionScope, OutputKind, ParameterDefinition, ParameterKind,
    PrivilegeRequirement, ResultParserKind, RiskLevel, TaskCategory, TaskDefinition,
    TaskImplementation, TaskStep,
};

const SUPPORTED_FAMILIES: [&str; 3] = ["debian", "rhel", "openeuler"];

mod containers;
mod helpers;
mod network;
mod security;
mod services;
mod storage;
mod system;
mod web;

pub fn built_in_catalog() -> Vec<TaskDefinition> {
    let mut catalog = legacy_catalog();
    catalog.retain(|definition| {
        !matches!(
            definition.category,
            TaskCategory::System | TaskCategory::Storage
        ) && definition.id != "service.status"
    });
    catalog.extend(system::tasks());
    catalog.extend(storage::tasks());
    catalog.extend(network::tasks());
    catalog.extend(security::tasks());
    catalog.extend(services::tasks());
    catalog.extend(web::tasks());
    catalog.extend(containers::tasks());

    let mut ids = BTreeSet::new();
    catalog.retain(|definition| {
        let inserted = ids.insert(definition.id.clone());
        debug_assert!(inserted, "内置任务 ID 重复：{}", definition.id);
        inserted
    });
    catalog
}

fn legacy_catalog() -> Vec<TaskDefinition> {
    let mut catalog = vec![
        task(TaskSpec {
            id: "system.overview",
            category: TaskCategory::System,
            title: "系统概览",
            description: "查看运行时间、负载、内存、磁盘、网络和进程摘要",
            risk_level: RiskLevel::Safe,
            parameters: Vec::new(),
            implementations: vec![implementation(
                "posix",
                &[],
                &["uptime", "uname", "df", "ps"],
                "printf '== system ==\\n'; uname -a; printf '== uptime ==\\n'; uptime; printf '== memory ==\\n'; (free -b 2>/dev/null || true); printf '== disk ==\\n'; df -P -B1; printf '== network ==\\n'; (ip -brief address 2>/dev/null || hostname -I 2>/dev/null || true); printf '== processes ==\\n'; ps -eo pid,user,%cpu,%mem,etime,args | head -n 21",
                ResultParserKind::KeyValue,
            )],
            output_kind: OutputKind::KeyValue,
        }),
        task(TaskSpec {
            id: "system.disk_usage",
            category: TaskCategory::System,
            title: "磁盘使用",
            description: "按字节查看挂载点容量和使用率",
            risk_level: RiskLevel::Safe,
            parameters: Vec::new(),
            implementations: vec![implementation(
                "posix",
                &[],
                &["df"],
                "df -P -B1",
                ResultParserKind::Table,
            )],
            output_kind: OutputKind::Table,
        }),
        task(TaskSpec {
            id: "system.process_query",
            category: TaskCategory::System,
            title: "进程查询",
            description: "按名称安全过滤进程",
            risk_level: RiskLevel::Safe,
            parameters: vec![
                string_parameter("query", "进程名称", "固定文本过滤词", 1, 128),
                integer_parameter("limit", "结果上限", "最多返回的进程数", 1, 200, 50),
            ],
            implementations: vec![implementation(
                "posix",
                &[],
                &["ps", "grep", "head"],
                "ps -eo pid,user,%cpu,%mem,etime,args | grep -F -- {{query}} | grep -v '[g]rep -F' | head -n {{limit}}",
                ResultParserKind::Table,
            )],
            output_kind: OutputKind::Table,
        }),
        service_task("service.status", "服务状态", "查看服务状态", RiskLevel::Safe, "status"),
        service_task("service.start", "启动服务", "启动指定服务", RiskLevel::Dangerous, "start"),
        service_task("service.stop", "停止服务", "停止指定服务", RiskLevel::Dangerous, "stop"),
        service_task(
            "service.restart",
            "重启服务",
            "重启指定服务",
            RiskLevel::Dangerous,
            "restart",
        ),
        task(TaskSpec {
            id: "logs.search",
            category: TaskCategory::Logs,
            title: "日志检索",
            description: "检索普通日志或 gzip 压缩日志并生成可下载结果",
            risk_level: RiskLevel::Caution,
            parameters: vec![
                ParameterDefinition {
                    name: "path".into(),
                    label: "日志路径".into(),
                    description: "远端绝对 .log 或 .gz 路径".into(),
                    kind: ParameterKind::AbsolutePath,
                    required: true,
                    default_value: None,
                    sensitive: false,
                },
                string_parameter("keyword", "关键词", "固定文本关键词", 1, 512),
                ParameterDefinition {
                    name: "caseSensitive".into(),
                    label: "区分大小写".into(),
                    description: "启用区分大小写匹配".into(),
                    kind: ParameterKind::Boolean,
                    required: false,
                    default_value: Some(json!(false)),
                    sensitive: false,
                },
                integer_parameter("context", "上下文", "匹配项前后行数", 0, 20, 2),
                integer_parameter("limit", "结果上限", "最多返回的匹配数", 1, 10_000, 1_000),
            ],
            implementations: vec![implementation(
                "grep",
                &[],
                &["grep"],
                "grep -n -F -- {{keyword}} {{path}} | head -n {{limit}}",
                ResultParserKind::Text,
            )],
            output_kind: OutputKind::LogMatches,
        }),
    ];
    let mut ids = BTreeSet::new();
    catalog.retain(|definition| {
        let inserted = ids.insert(definition.id.clone());
        debug_assert!(inserted, "内置任务 ID 重复：{}", definition.id);
        inserted
    });
    catalog
}

fn service_task(
    id: &str,
    title: &str,
    description: &str,
    risk_level: RiskLevel,
    action: &str,
) -> TaskDefinition {
    task(TaskSpec {
        id,
        category: TaskCategory::Service,
        title,
        description,
        risk_level,
        parameters: vec![ParameterDefinition {
            name: "service".into(),
            label: "服务名".into(),
            description: "systemd 单元或传统服务名".into(),
            kind: ParameterKind::ServiceName,
            required: true,
            default_value: None,
            sensitive: false,
        }],
        implementations: vec![
            service_implementation("systemd", "systemctl", action),
            service_implementation("service", "service", action),
        ],
        output_kind: OutputKind::Text,
    })
}

struct TaskSpec<'a> {
    id: &'a str,
    category: TaskCategory,
    title: &'a str,
    description: &'a str,
    risk_level: RiskLevel,
    parameters: Vec<ParameterDefinition>,
    implementations: Vec<TaskImplementation>,
    output_kind: OutputKind,
}

fn task(spec: TaskSpec<'_>) -> TaskDefinition {
    let scope = if spec.risk_level == RiskLevel::Safe {
        ExecutionScope::ReadOnlyBatch
    } else {
        ExecutionScope::SingleServer
    };
    TaskDefinition {
        id: spec.id.into(),
        version: 2,
        category: spec.category,
        title: spec.title.into(),
        description: spec.description.into(),
        risk_level: spec.risk_level,
        estimated_seconds: 30,
        privilege: PrivilegeRequirement::CurrentUser,
        scope,
        parameters: spec.parameters,
        implementations: spec.implementations,
        output_kind: spec.output_kind,
    }
}

fn implementation(
    id: &str,
    service_managers: &[&str],
    required_commands: &[&str],
    command_template: &str,
    result_parser: ResultParserKind,
) -> TaskImplementation {
    TaskImplementation {
        id: id.into(),
        compatibility: compatibility(service_managers, required_commands),
        preflight_steps: Vec::new(),
        preview_steps: vec![task_step("preview", "执行预演", command_template)],
        backup_plan: None,
        execution_steps: vec![task_step("execute", "执行任务", command_template)],
        verify_steps: Vec::new(),
        rollback_plan: None,
        result_parser,
    }
}

fn service_implementation(
    service_manager: &str,
    command: &str,
    action: &str,
) -> TaskImplementation {
    let command_template = if command == "systemctl" {
        format!("systemctl {action} -- {{{{service}}}}")
    } else {
        format!("service {{{{service}}}} {action}")
    };
    let preview_template = if command == "systemctl" {
        "systemctl status -- {{service}}".to_owned()
    } else {
        "service {{service}} status".to_owned()
    };

    TaskImplementation {
        id: format!("{service_manager}-{action}"),
        compatibility: compatibility(&[service_manager], &[command]),
        preflight_steps: Vec::new(),
        preview_steps: vec![task_step("preview", "查看当前服务状态", &preview_template)],
        backup_plan: None,
        execution_steps: vec![task_step("execute", "执行任务", &command_template)],
        verify_steps: Vec::new(),
        rollback_plan: None,
        result_parser: ResultParserKind::ServiceStatus,
    }
}

fn task_step(id: &str, title: &str, command_template: &str) -> TaskStep {
    TaskStep {
        id: id.into(),
        title: title.into(),
        timeout_seconds: 30,
        output_limit_bytes: 1024 * 1024,
        command_template: command_template.into(),
    }
}

fn compatibility(service_managers: &[&str], required_commands: &[&str]) -> CompatibilityPredicate {
    CompatibilityPredicate {
        os_families: SUPPORTED_FAMILIES
            .iter()
            .map(|value| (*value).into())
            .collect(),
        service_managers: service_managers
            .iter()
            .map(|value| (*value).into())
            .collect(),
        required_commands: required_commands
            .iter()
            .map(|value| (*value).into())
            .collect(),
    }
}

fn string_parameter(
    name: &str,
    label: &str,
    description: &str,
    min_length: usize,
    max_length: usize,
) -> ParameterDefinition {
    ParameterDefinition {
        name: name.into(),
        label: label.into(),
        description: description.into(),
        kind: ParameterKind::String {
            min_length,
            max_length,
            multiline: false,
        },
        required: true,
        default_value: None,
        sensitive: false,
    }
}

fn integer_parameter(
    name: &str,
    label: &str,
    description: &str,
    min: i64,
    max: i64,
    default: i64,
) -> ParameterDefinition {
    ParameterDefinition {
        name: name.into(),
        label: label.into(),
        description: description.into(),
        kind: ParameterKind::Integer { min, max },
        required: false,
        default_value: Some(json!(default)),
        sensitive: false,
    }
}
