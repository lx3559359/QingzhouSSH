use std::{collections::BTreeMap, path::Path};

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
    Ok(ValidatedParameters { values })
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
            if !value.starts_with('/') || !Path::new(value).is_absolute() {
                return Err(invalid(parameter));
            }
            Ok(shell_quote(value))
        }
        ParameterKind::ServiceName => {
            let value = required_string(parameter, value)?;
            if value.is_empty()
                || value.len() > 255
                || !value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'@')
                })
            {
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
    }
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

fn invalid(parameter: &ParameterDefinition) -> AppError {
    AppError::Validation(format!("任务参数 {} 无效", parameter.name))
}

pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
