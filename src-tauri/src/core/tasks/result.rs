use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    core::{redaction::Redactor, tasks::model::ResultParserKind},
    domain::events::truncate_utf8,
    error::AppResult,
};

const MAX_TECHNICAL_DETAILS_BYTES: usize = 64 * 1024;
const MAX_FINDINGS: usize = 100;
const MAX_SUGGESTIONS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationConclusion {
    Normal,
    Warning,
    Failed,
    Uncertain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationFinding {
    pub level: FindingLevel,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult {
    pub status: OperationConclusion,
    pub summary: String,
    pub findings: Vec<OperationFinding>,
    pub suggestions: Vec<String>,
    pub technical_details: String,
}

pub fn parse_result(
    parser: ResultParserKind,
    raw: &str,
    redactor: &Redactor,
) -> AppResult<OperationResult> {
    let technical_details = truncate_utf8(redactor.redact(raw), MAX_TECHNICAL_DETAILS_BYTES);
    let mut metrics = BTreeMap::new();
    let mut warning_codes = BTreeSet::new();
    let mut error_codes = BTreeSet::new();
    let mut unsupported = BTreeSet::new();

    for line in technical_details.lines() {
        if let Some(value) = line.strip_prefix("__QZ_METRIC__ ") {
            if let Some((key, value)) = value.split_once('=') {
                if valid_marker_token(key) && value.len() <= 256 {
                    metrics.insert(key.to_owned(), value.to_owned());
                }
            }
        } else if let Some(code) = line.strip_prefix("__QZ_WARNING__ ") {
            if known_warning(code) {
                warning_codes.insert(code.to_owned());
            }
        } else if let Some(code) = line.strip_prefix("__QZ_ERROR__ ") {
            if known_error(code) {
                error_codes.insert(code.to_owned());
            }
        } else if let Some(capability) = line.strip_prefix("__QZ_UNSUPPORTED__ ") {
            if valid_marker_token(capability) {
                unsupported.insert(capability.to_owned());
            }
        }
    }

    if metrics
        .get("disk_percent")
        .and_then(|value| value.parse::<u8>().ok())
        .is_some_and(|value| value >= 90)
    {
        warning_codes.insert("disk_usage".into());
    }

    let udp_inconclusive = parser == ResultParserKind::NetworkProbe
        && technical_details
            .lines()
            .any(|line| line.trim() == "probe=no_response");
    if parser == ResultParserKind::ServiceStatus
        && (technical_details.contains("ActiveState=failed")
            || technical_details.contains("ActiveState=inactive"))
    {
        warning_codes.insert("service_failed".into());
    }
    if parser == ResultParserKind::ContainerStatus
        && technical_details.to_ascii_lowercase().contains("unhealthy")
    {
        warning_codes.insert("container_unhealthy".into());
    }

    let mut findings = Vec::new();
    let mut suggestions = Vec::new();
    for code in &error_codes {
        push_finding(
            &mut findings,
            FindingLevel::Error,
            error_title(code),
            "后端采集器报告检查失败。",
        );
    }
    for code in &warning_codes {
        let (title, detail, suggestion) = warning_guidance(code);
        push_finding(&mut findings, FindingLevel::Warning, title, detail);
        push_suggestion(&mut suggestions, suggestion);
    }
    for capability in &unsupported {
        push_finding(
            &mut findings,
            FindingLevel::Info,
            "部分检查不可用",
            &format!("服务器缺少或无权使用：{capability}"),
        );
        push_suggestion(
            &mut suggestions,
            "可在确认服务器策略后安装对应只读诊断工具，或请有权限的人员复核。",
        );
    }
    if udp_inconclusive {
        push_finding(
            &mut findings,
            FindingLevel::Info,
            "UDP 探测无响应",
            "UDP 无响应既可能是不可达，也可能是服务按协议不回复空探测。",
        );
        push_suggestion(
            &mut suggestions,
            "结合服务端监听状态、防火墙规则和真实业务请求继续确认。",
        );
    }

    let (status, summary) = if !error_codes.is_empty() {
        (
            OperationConclusion::Failed,
            "诊断执行失败，请查看错误详情。",
        )
    } else if udp_inconclusive {
        (
            OperationConclusion::Uncertain,
            "探测未收到响应，当前无法确认网络是否可达。",
        )
    } else if warning_codes.contains("disk_usage") {
        (
            OperationConclusion::Warning,
            "发现磁盘使用率偏高，需要尽快处理。",
        )
    } else if !warning_codes.is_empty() {
        (OperationConclusion::Warning, "诊断发现需要关注的问题。")
    } else if !unsupported.is_empty() {
        (
            OperationConclusion::Uncertain,
            "部分检查能力不可用，当前结论不完整。",
        )
    } else {
        (OperationConclusion::Normal, "诊断已完成，未发现明确异常。")
    };

    Ok(OperationResult {
        status,
        summary: summary.into(),
        findings,
        suggestions,
        technical_details,
    })
}

fn valid_marker_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn known_warning(code: &str) -> bool {
    matches!(
        code,
        "disk_usage"
            | "cpu_pressure"
            | "memory_pressure"
            | "oom_events"
            | "connectivity_failed"
            | "udp_inconclusive"
            | "service_failed"
            | "container_unhealthy"
            | "firewall_exposure"
            | "time_drift"
    )
}

fn known_error(code: &str) -> bool {
    matches!(
        code,
        "process_not_found"
            | "packet_capture_failed"
            | "capture_too_large"
            | "invalid_container_action"
    )
}

fn error_title(code: &str) -> &'static str {
    match code {
        "process_not_found" => "进程不存在",
        "packet_capture_failed" => "抓包失败",
        "capture_too_large" => "抓包超过大小限制",
        "invalid_container_action" => "容器操作无效",
        _ => "诊断失败",
    }
}

fn warning_guidance(code: &str) -> (&'static str, &'static str, &'static str) {
    match code {
        "disk_usage" => (
            "磁盘空间偏高",
            "一个或多个文件系统的使用率达到告警范围。",
            "优先清理无用日志、缓存或过期备份，再确认磁盘增长来源。",
        ),
        "cpu_pressure" => (
            "CPU 压力偏高",
            "负载或 CPU 压力指标需要关注。",
            "查看 CPU 排行中的进程，并结合业务峰值判断是否扩容或限流。",
        ),
        "memory_pressure" | "oom_events" => (
            "内存压力异常",
            "检测到内存压力或 OOM 相关线索。",
            "查看内存排行和 OOM 时间点，确认是否存在泄漏或内存配额不足。",
        ),
        "connectivity_failed" => (
            "网络连通性异常",
            "目标主机未通过基础连通性探测。",
            "检查目标地址、路由、安全组和防火墙后重试。",
        ),
        "udp_inconclusive" => (
            "UDP 结果不确定",
            "UDP 空探测没有收到响应。",
            "结合服务端监听、防火墙和真实协议请求继续确认。",
        ),
        "service_failed" => (
            "服务状态异常",
            "服务处于失败或未运行状态。",
            "先查看服务日志和退出码，再决定是否重启。",
        ),
        "container_unhealthy" => (
            "容器健康异常",
            "容器运行时报告 unhealthy。",
            "查看容器健康检查、日志和资源限制。",
        ),
        "firewall_exposure" => (
            "网络暴露需复核",
            "监听端口与防火墙规则可能不一致。",
            "逐项确认对公网开放是否符合预期。",
        ),
        "time_drift" => (
            "时间同步异常",
            "系统时钟或时间源状态需要关注。",
            "检查 NTP/Chrony 配置和上游时间源连通性。",
        ),
        _ => (
            "诊断告警",
            "检查结果包含需要关注的机器标记。",
            "查看技术详情并请熟悉该服务的人员复核。",
        ),
    }
}

fn push_finding(
    findings: &mut Vec<OperationFinding>,
    level: FindingLevel,
    title: &str,
    detail: &str,
) {
    if findings.len() < MAX_FINDINGS {
        findings.push(OperationFinding {
            level,
            title: title.into(),
            detail: detail.into(),
        });
    }
}

fn push_suggestion(suggestions: &mut Vec<String>, suggestion: &str) {
    if suggestions.len() < MAX_SUGGESTIONS
        && !suggestions.iter().any(|existing| existing == suggestion)
    {
        suggestions.push(suggestion.into());
    }
}
