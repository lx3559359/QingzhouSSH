use std::{
    collections::{BTreeMap, BTreeSet},
    net::IpAddr,
};

use serde_json::Value;

use crate::{
    core::tasks::model::{ParameterDefinition, ParameterKind, TaskDefinition},
    error::{AppError, AppResult},
};

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedParameter {
    pub value: Value,
    pub shell_value: String,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ValidatedParameters {
    values: BTreeMap<String, ValidatedParameter>,
}

impl ValidatedParameters {
    pub fn get(&self, name: &str) -> Option<&ValidatedParameter> {
        self.values.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &ValidatedParameter)> {
        self.values.iter()
    }
}

pub fn validate_parameters(
    definition: &TaskDefinition,
    input: &Value,
) -> AppResult<ValidatedParameters> {
    let object = input
        .as_object()
        .ok_or_else(|| AppError::Validation("任务参数必须是对象".into()))?;
    for key in object.keys() {
        if !definition
            .parameters
            .iter()
            .any(|parameter| parameter.name == *key)
        {
            return Err(AppError::Validation(format!("存在未知任务参数：{key}")));
        }
    }

    let mut values = BTreeMap::new();
    for parameter in &definition.parameters {
        let value = object
            .get(&parameter.name)
            .cloned()
            .or_else(|| parameter.default_value.clone());
        let Some(value) = value else {
            if parameter.required {
                return Err(AppError::Validation(format!(
                    "缺少任务参数：{}",
                    parameter.name
                )));
            }
            continue;
        };
        let shell_value = validate_value(parameter, &value)?;
        values.insert(
            parameter.name.clone(),
            ValidatedParameter {
                value,
                shell_value,
                sensitive: parameter.sensitive,
            },
        );
    }
    validate_task_constraints(definition, &values)?;
    Ok(ValidatedParameters { values })
}

fn validate_task_constraints(
    definition: &TaskDefinition,
    values: &BTreeMap<String, ValidatedParameter>,
) -> AppResult<()> {
    let string_value = |name: &str| {
        values
            .get(name)
            .and_then(|parameter| parameter.value.as_str())
    };

    match definition.id.as_str() {
        "storage.swap_manage" => {
            let path = string_value("path")
                .ok_or_else(|| AppError::Validation("Swap 文件路径无效".into()))?;
            let managed_path = path == "/swapfile"
                || path
                    .strip_prefix("/var/lib/qingzhou/swap/")
                    .is_some_and(is_safe_relative_path);
            if !managed_path {
                return Err(AppError::Validation(
                    "Swap 文件只允许使用 /swapfile 或 /var/lib/qingzhou/swap/ 下的路径".into(),
                ));
            }
        }
        "security.file_permissions" => {
            let path = string_value("path")
                .ok_or_else(|| AppError::Validation("目标路径无效".into()))?;
            const PROTECTED_PATHS: &[&str] = &[
                "/", "/bin", "/boot", "/dev", "/etc", "/home", "/lib", "/lib64", "/proc",
                "/run", "/sbin", "/sys", "/usr", "/var",
            ];
            if PROTECTED_PATHS.contains(&path) || !is_normalized_absolute_path(path) {
                return Err(AppError::Validation(
                    "不能直接修改系统顶层目录；请选择具体文件或下级目录".into(),
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_value(parameter: &ParameterDefinition, value: &Value) -> AppResult<String> {
    match &parameter.kind {
        ParameterKind::String {
            min_length,
            max_length,
            multiline,
        } => {
            let value = required_string(parameter, value)?;
            let length = value.chars().count();
            if length < *min_length || length > *max_length || (!multiline && value.contains('\n'))
            {
                return Err(invalid(parameter));
            }
            reject_nul(parameter, value)?;
            Ok(shell_quote(value))
        }
        ParameterKind::Integer { min, max } => {
            let value = value.as_i64().ok_or_else(|| invalid(parameter))?;
            if value < *min || value > *max {
                return Err(invalid(parameter));
            }
            Ok(value.to_string())
        }
        ParameterKind::Boolean => value
            .as_bool()
            .map(|value| if value { "true" } else { "false" }.into())
            .ok_or_else(|| invalid(parameter)),
        ParameterKind::Enum { options } => {
            let value = required_string(parameter, value)?;
            if !options.iter().any(|option| option == value) {
                return Err(invalid(parameter));
            }
            Ok(shell_quote(value))
        }
        ParameterKind::AbsolutePath => {
            let value = required_string(parameter, value)?;
            reject_nul(parameter, value)?;
            if !value.starts_with('/') {
                return Err(invalid(parameter));
            }
            Ok(shell_quote(value))
        }
        ParameterKind::Timezone => {
            let value = required_string(parameter, value)?;
            if !is_timezone(value) {
                return Err(invalid(parameter));
            }
            Ok(shell_quote(value))
        }
        ParameterKind::ServiceName => {
            let value = required_string(parameter, value)?;
            if !is_service_name(value) {
                return Err(invalid(parameter));
            }
            Ok(shell_quote(value))
        }
        ParameterKind::TimeRange => {
            let object = value.as_object().ok_or_else(|| invalid(parameter))?;
            if object.keys().any(|key| key != "start" && key != "end") {
                return Err(invalid(parameter));
            }
            let start = object
                .get("start")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid(parameter))?;
            let end = object
                .get("end")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid(parameter))?;
            if !is_time_value(start) || !is_time_value(end) {
                return Err(invalid(parameter));
            }
            Ok(shell_quote(&format!("{start}/{end}")))
        }
        ParameterKind::Host => {
            let value = required_string(parameter, value)?;
            if !is_host(value) {
                return Err(invalid(parameter));
            }
            Ok(shell_quote(value))
        }
        ParameterKind::Port => {
            let value = value.as_u64().ok_or_else(|| invalid(parameter))?;
            if !(1..=65_535).contains(&value) {
                return Err(invalid(parameter));
            }
            Ok(shell_quote(&value.to_string()))
        }
        ParameterKind::InterfaceName => {
            let value = required_string(parameter, value)?;
            if value.is_empty()
                || value.len() > 32
                || !value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'_' | b'-')
                })
            {
                return Err(invalid(parameter));
            }
            Ok(shell_quote(value))
        }
        ParameterKind::Cidr => {
            let value = required_string(parameter, value)?;
            if !is_cidr(value) {
                return Err(invalid(parameter));
            }
            Ok(shell_quote(value))
        }
        ParameterKind::ContainerName => {
            let value = required_string(parameter, value)?;
            if value.is_empty()
                || value.len() > 128
                || !value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':')
                })
            {
                return Err(invalid(parameter));
            }
            Ok(shell_quote(value))
        }
        ParameterKind::FileMode => {
            let value = required_string(parameter, value)?;
            if value.len() != 4
                || !value.starts_with('0')
                || !value.bytes().all(|byte| matches!(byte, b'0'..=b'7'))
            {
                return Err(invalid(parameter));
            }
            Ok(shell_quote(value))
        }
        ParameterKind::CronExpression => {
            let value = required_string(parameter, value)?;
            let fields = value.split_whitespace().collect::<Vec<_>>();
            if fields.len() != 5
                || !fields.iter().all(|field| {
                    !field.is_empty()
                        && field.bytes().all(|byte| {
                            byte.is_ascii_digit() || matches!(byte, b'*' | b',' | b'-' | b'/')
                        })
                })
            {
                return Err(invalid(parameter));
            }
            Ok(shell_quote(value))
        }
        ParameterKind::MultiSelect { options, max_items } => {
            let values = value.as_array().ok_or_else(|| invalid(parameter))?;
            if values.is_empty() || values.len() > *max_items {
                return Err(invalid(parameter));
            }
            let mut seen = BTreeSet::new();
            let mut shell_values = Vec::with_capacity(values.len());
            for value in values {
                let value = required_string(parameter, value)?;
                if !options.iter().any(|option| option == value) || !seen.insert(value) {
                    return Err(invalid(parameter));
                }
                shell_values.push(shell_quote(value));
            }
            Ok(shell_values.join(" "))
        }
        ParameterKind::ServiceMultiSelect { max_items } => {
            let values = value.as_array().ok_or_else(|| invalid(parameter))?;
            if values.is_empty() || values.len() > *max_items {
                return Err(invalid(parameter));
            }
            let mut seen = BTreeSet::new();
            let mut shell_values = Vec::with_capacity(values.len());
            for value in values {
                let value = required_string(parameter, value)?;
                if !is_service_name(value) || !seen.insert(value) {
                    return Err(invalid(parameter));
                }
                shell_values.push(shell_quote(value));
            }
            Ok(shell_values.join(" "))
        }
    }
}

fn is_service_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'@'))
}

fn is_timezone(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+' | b'-')
                })
        })
}

fn is_safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn is_normalized_absolute_path(value: &str) -> bool {
    value.starts_with('/')
        && (value == "/"
            || value[1..]
                .split('/')
                .all(|segment| !segment.is_empty() && segment != "." && segment != ".."))
}

pub fn script_parameter_env_name(name: &str) -> AppResult<String> {
    if name.is_empty()
        || name.len() > 32
        || name.starts_with("QZ_")
        || !name.as_bytes()[0].is_ascii_uppercase()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(AppError::Validation("脚本参数名称无效".into()));
    }
    Ok(format!("QZ_PARAM_{name}"))
}

fn required_string<'a>(parameter: &ParameterDefinition, value: &'a Value) -> AppResult<&'a str> {
    value.as_str().ok_or_else(|| invalid(parameter))
}

fn reject_nul(parameter: &ParameterDefinition, value: &str) -> AppResult<()> {
    if value.contains('\0') {
        Err(invalid(parameter))
    } else {
        Ok(())
    }
}

fn is_time_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_digit() || matches!(byte, b'-' | b':' | b'.' | b'+' | b'T' | b'Z' | b' ')
        })
}

fn is_host(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':'))
        && value
            .split('.')
            .all(|label| !label.is_empty() && !label.starts_with('-') && !label.ends_with('-'))
}

fn is_cidr(value: &str) -> bool {
    let Some((address, prefix)) = value.split_once('/') else {
        return false;
    };
    if prefix.contains('/') {
        return false;
    }
    let Ok(address) = address.parse::<IpAddr>() else {
        return false;
    };
    let Ok(prefix) = prefix.parse::<u16>() else {
        return false;
    };
    match address {
        IpAddr::V4(_) => prefix <= 32,
        IpAddr::V6(_) => prefix <= 128,
    }
}

fn invalid(parameter: &ParameterDefinition) -> AppError {
    AppError::Validation(format!("任务参数 {} 无效", parameter.name))
}

pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
