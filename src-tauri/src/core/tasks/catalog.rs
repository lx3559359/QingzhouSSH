use serde_json::json;

use crate::core::tasks::model::{
    CompatibilityPredicate, OutputKind, ParameterDefinition, ParameterKind, RiskLevel,
    TaskCategory, TaskDefinition, TaskImplementation,
};

const SUPPORTED_FAMILIES: [&str; 3] = ["debian", "rhel", "openeuler"];

pub fn built_in_catalog() -> Vec<TaskDefinition> {
    vec![
        task(
            "system.overview",
            TaskCategory::System,
            "系统概览",
            "查看运行时间、负载、内存、磁盘、网络和进程摘要",
            RiskLevel::Safe,
            Vec::new(),
            vec![implementation(
                "posix",
                &[],
                &["uptime", "uname", "df", "ps"],
                "printf '== system ==\\n'; uname -a; printf '== uptime ==\\n'; uptime; printf '== memory ==\\n'; (free -b 2>/dev/null || true); printf '== disk ==\\n'; df -P -B1; printf '== network ==\\n'; (ip -brief address 2>/dev/null || hostname -I 2>/dev/null || true); printf '== processes ==\\n'; ps -eo pid,user,%cpu,%mem,etime,args | head -n 21",
            )],
            OutputKind::KeyValue,
        ),
        task(
            "system.disk_usage",
            TaskCategory::System,
            "磁盘使用",
            "按字节查看挂载点容量和使用率",
            RiskLevel::Safe,
            Vec::new(),
            vec![implementation("posix", &[], &["df"], "df -P -B1")],
            OutputKind::Table,
        ),
        task(
            "system.process_query",
            TaskCategory::System,
            "进程查询",
            "按名称安全过滤进程",
            RiskLevel::Safe,
            vec![
                string_parameter("query", "进程名称", "固定文本过滤词", 1, 128),
                integer_parameter("limit", "结果上限", "最多返回的进程数", 1, 200, 50),
            ],
            vec![implementation(
                "posix",
                &[],
                &["ps", "grep", "head"],
                "ps -eo pid,user,%cpu,%mem,etime,args | grep -F -- {{query}} | grep -v '[g]rep -F' | head -n {{limit}}",
            )],
            OutputKind::Table,
        ),
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
        task(
            "logs.search",
            TaskCategory::Logs,
            "日志检索",
            "检索普通日志或 gzip 压缩日志并生成可下载结果",
            RiskLevel::Caution,
            vec![
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
            vec![implementation(
                "grep",
                &[],
                &["grep"],
                "grep -n -F -- {{keyword}} {{path}} | head -n {{limit}}",
            )],
            OutputKind::LogMatches,
        ),
    ]
}

fn service_task(
    id: &str,
    title: &str,
    description: &str,
    risk_level: RiskLevel,
    action: &str,
) -> TaskDefinition {
    task(
        id,
        TaskCategory::Service,
        title,
        description,
        risk_level,
        vec![ParameterDefinition {
            name: "service".into(),
            label: "服务名".into(),
            description: "systemd 单元或传统服务名".into(),
            kind: ParameterKind::ServiceName,
            required: true,
            default_value: None,
            sensitive: false,
        }],
        vec![
            TaskImplementation {
                id: format!("systemd-{action}"),
                compatibility: compatibility(&["systemd"], &["systemctl"]),
                command_template: format!("systemctl {action} -- {{{{service}}}}"),
            },
            TaskImplementation {
                id: format!("sysv-{action}"),
                compatibility: compatibility(&["service"], &["service"]),
                command_template: format!("service {{{{service}}}} {action}"),
            },
        ],
        OutputKind::Text,
    )
}

fn task(
    id: &str,
    category: TaskCategory,
    title: &str,
    description: &str,
    risk_level: RiskLevel,
    parameters: Vec<ParameterDefinition>,
    implementations: Vec<TaskImplementation>,
    output_kind: OutputKind,
) -> TaskDefinition {
    TaskDefinition {
        id: id.into(),
        version: 1,
        category,
        title: title.into(),
        description: description.into(),
        risk_level,
        parameters,
        implementations,
        output_kind,
    }
}

fn implementation(
    id: &str,
    service_managers: &[&str],
    required_commands: &[&str],
    command_template: &str,
) -> TaskImplementation {
    TaskImplementation {
        id: id.into(),
        compatibility: compatibility(service_managers, required_commands),
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
