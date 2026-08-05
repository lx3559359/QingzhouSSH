use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{
    core::tasks::{
        script_parameter_env_name, validate_parameters, ExecutionScope, OutputKind,
        ParameterDefinition, ParameterKind, PrivilegeRequirement, RiskLevel, TaskCategory,
        TaskDefinition, ValidatedParameters,
    },
    error::{AppError, AppResult},
};

pub const PERSONAL_SCRIPT_RISK: RiskLevel = RiskLevel::Dangerous;
pub const PERSONAL_SCRIPT_AUTOMATIC_ROLLBACK_AVAILABLE: bool = false;
pub const MAX_SCRIPT_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_SCRIPT_PARAMETERS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptScanWarning {
    pub code: String,
    pub message: String,
    pub line_number: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptScanSummary {
    pub line_count: usize,
    pub character_count: usize,
    pub body_sha256: String,
    pub warning_count: usize,
    pub warnings: Vec<ScriptScanWarning>,
}

pub fn validate_script_metadata(title: &str, category: &str, tags: &[String]) -> AppResult<()> {
    validate_text(title, 1, 80, "脚本标题")?;
    validate_text(category, 1, 40, "脚本分类")?;
    if tags.len() > 20 {
        return Err(AppError::Validation("脚本标签不能超过 20 个".into()));
    }
    let mut unique = BTreeSet::new();
    for tag in tags {
        validate_text(tag, 1, 24, "脚本标签")?;
        if !unique.insert(tag) {
            return Err(AppError::Validation("脚本标签不能重复".into()));
        }
    }
    Ok(())
}

pub fn validate_script_body(body: &str) -> AppResult<()> {
    if body.is_empty() || body.len() > MAX_SCRIPT_BODY_BYTES || body.contains('\0') {
        return Err(AppError::Validation(
            "脚本正文必须为 1 字节到 1 MiB 且不能包含 NUL 字符".into(),
        ));
    }
    Ok(())
}

pub fn validate_script_timeout(timeout_seconds: u64) -> AppResult<()> {
    if (1..=3600).contains(&timeout_seconds) {
        Ok(())
    } else {
        Err(AppError::Validation(
            "脚本超时时间必须在 1 到 3600 秒之间".into(),
        ))
    }
}

pub fn validate_parameter_name(name: &str) -> AppResult<String> {
    script_parameter_env_name(name)
}

pub fn validate_script_parameters(parameters: &[ParameterDefinition]) -> AppResult<()> {
    if parameters.len() > MAX_SCRIPT_PARAMETERS {
        return Err(AppError::Validation("脚本参数不能超过 32 个".into()));
    }
    let mut names = BTreeSet::new();
    for parameter in parameters {
        validate_parameter_name(&parameter.name)?;
        if !names.insert(parameter.name.as_str()) {
            return Err(AppError::Validation("脚本参数名称不能重复".into()));
        }
        validate_chinese_label(&parameter.label)?;
        if parameter.description.chars().count() > 500 || parameter.description.contains('\0') {
            return Err(AppError::Validation("脚本参数说明超过长度限制".into()));
        }
        validate_parameter_kind(&parameter.kind)?;
        if let Some(default) = parameter.default_value.clone() {
            validate_parameter_default(parameter, default)?;
        }
    }
    Ok(())
}

pub fn validate_script_parameter_values(
    parameters: &[ParameterDefinition],
    input: &Value,
) -> AppResult<ValidatedParameters> {
    validate_script_parameters(parameters)?;
    let task = parameter_validation_task(parameters.to_vec());
    validate_parameters(&task, input)
}

pub fn scan_script_body(body: &str) -> AppResult<ScriptScanSummary> {
    validate_script_body(body)?;
    let mut warnings = Vec::new();
    for (index, line) in body.lines().enumerate() {
        let lower = line.to_ascii_lowercase();
        let line_number = index + 1;
        push_warning(
            &mut warnings,
            lower.contains("rm -rf") || lower.contains("rm -fr"),
            "recursive_delete",
            "检测到递归强制删除，请确认目标路径和影响范围",
            line_number,
        );
        push_warning(
            &mut warnings,
            (lower.contains("dd ") && lower.contains("of=/dev/"))
                || lower.contains("mkfs")
                || lower.contains("> /dev/"),
            "disk_write",
            "检测到可能直接写入磁盘设备的命令",
            line_number,
        );
        push_warning(
            &mut warnings,
            lower.contains("passwd") || lower.contains("chpasswd") || lower.contains("useradd"),
            "account_change",
            "检测到用户或密码相关操作",
            line_number,
        );
        push_warning(
            &mut warnings,
            lower.contains("ip address")
                || lower.contains("ip addr")
                || lower.contains("ip route")
                || lower.contains("nmcli")
                || lower.contains("netplan"),
            "network_change",
            "检测到网络配置相关操作，可能导致连接中断",
            line_number,
        );
        push_warning(
            &mut warnings,
            lower.contains("firewall-cmd") || lower.contains("iptables") || lower.contains("nft "),
            "firewall_change",
            "检测到防火墙规则相关操作",
            line_number,
        );
        push_warning(
            &mut warnings,
            lower.contains("systemctl stop") || lower.contains("systemctl disable"),
            "service_stop",
            "检测到停止或禁用服务的操作",
            line_number,
        );
        push_warning(
            &mut warnings,
            (lower.contains("curl ") || lower.contains("wget "))
                && (lower.contains("| sh") || lower.contains("| bash")),
            "download_execute",
            "检测到下载后直接交给 Shell 执行的操作",
            line_number,
        );
        push_warning(
            &mut warnings,
            lower.contains("eval ") || lower.trim_start().starts_with("eval\t"),
            "dynamic_eval",
            "检测到动态 eval 执行，运行内容可能难以预判",
            line_number,
        );
    }
    warnings.truncate(64);
    Ok(ScriptScanSummary {
        line_count: body.lines().count(),
        character_count: body.chars().count(),
        body_sha256: format!("{:x}", Sha256::digest(body.as_bytes())),
        warning_count: warnings.len(),
        warnings,
    })
}

fn validate_text(value: &str, min: usize, max: usize, name: &str) -> AppResult<()> {
    let length = value.chars().count();
    if length < min || length > max || value.contains('\0') || value.trim() != value {
        Err(AppError::Validation(format!("{name}长度或格式无效")))
    } else {
        Ok(())
    }
}

fn validate_chinese_label(label: &str) -> AppResult<()> {
    validate_text(label, 1, 40, "脚本参数名称")?;
    if label.chars().any(is_cjk) {
        Ok(())
    } else {
        Err(AppError::Validation("脚本参数必须提供中文名称".into()))
    }
}

fn is_cjk(value: char) -> bool {
    matches!(value, '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}')
}

fn validate_parameter_kind(kind: &ParameterKind) -> AppResult<()> {
    match kind {
        ParameterKind::String {
            min_length,
            max_length,
            ..
        } if min_length <= max_length && *max_length <= 4096 => Ok(()),
        ParameterKind::Integer { min, max } if min <= max => Ok(()),
        ParameterKind::Boolean
        | ParameterKind::Host
        | ParameterKind::Port
        | ParameterKind::ServiceName
        | ParameterKind::ContainerName
        | ParameterKind::AbsolutePath => Ok(()),
        ParameterKind::Enum { options }
            if !options.is_empty()
                && options.len() <= 100
                && options.iter().all(|option| {
                    !option.is_empty() && option.len() <= 256 && !option.contains('\0')
                }) =>
        {
            Ok(())
        }
        _ => Err(AppError::Validation("个人脚本不支持该参数类型".into())),
    }
}

fn validate_parameter_default(parameter: &ParameterDefinition, default: Value) -> AppResult<()> {
    let mut definition = parameter.clone();
    definition.required = true;
    definition.default_value = None;
    let mut input = Map::new();
    input.insert(parameter.name.clone(), default);
    let task = parameter_validation_task(vec![definition]);
    validate_parameters(&task, &Value::Object(input)).map(|_| ())
}

fn parameter_validation_task(parameters: Vec<ParameterDefinition>) -> TaskDefinition {
    TaskDefinition {
        id: "script.personal.validation".into(),
        version: 1,
        category: TaskCategory::System,
        title: "脚本参数校验".into(),
        description: String::new(),
        risk_level: RiskLevel::Dangerous,
        estimated_seconds: 1,
        privilege: PrivilegeRequirement::CurrentUser,
        scope: ExecutionScope::SingleServer,
        parameters,
        implementations: Vec::new(),
        output_kind: OutputKind::Text,
    }
}

fn push_warning(
    warnings: &mut Vec<ScriptScanWarning>,
    matched: bool,
    code: &str,
    message: &str,
    line_number: usize,
) {
    if matched && !warnings.iter().any(|warning| warning.code == code) {
        warnings.push(ScriptScanWarning {
            code: code.into(),
            message: message.into(),
            line_number,
        });
    }
}
