use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    domain::workflow::{EqualityOperator, NumericOperator, WorkflowCondition},
    error::{AppError, AppResult},
};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConditionContext {
    pub exit_code: Option<i32>,
    pub result: Option<Value>,
    pub output_summary: Option<String>,
}

pub fn validate_condition(condition: &WorkflowCondition) -> AppResult<()> {
    match condition {
        WorkflowCondition::ExitCode { .. } => Ok(()),
        WorkflowCondition::ResultField { path, value, .. } => {
            validate_result_path(path)?;
            if !matches!(value, Value::String(_) | Value::Number(_) | Value::Bool(_)) {
                return Err(AppError::Validation(
                    "条件比较值只能是字符串、数字或布尔值".into(),
                ));
            }
            Ok(())
        }
        WorkflowCondition::OutputContains { text, .. } => {
            if text.is_empty() || text.len() > 512 || text.contains('\0') {
                return Err(AppError::Validation(
                    "输出条件文本必须为 1 到 512 字节".into(),
                ));
            }
            Ok(())
        }
    }
}

pub fn evaluate_condition(
    condition: &WorkflowCondition,
    context: &ConditionContext,
) -> AppResult<bool> {
    validate_condition(condition)?;
    match condition {
        WorkflowCondition::ExitCode { operator, value } => {
            let actual = context
                .exit_code
                .ok_or_else(|| AppError::Validation("条件来源没有退出码".into()))?;
            Ok(compare_number(actual, *operator, *value))
        }
        WorkflowCondition::ResultField {
            path,
            operator,
            value,
        } => {
            let mut actual = context
                .result
                .as_ref()
                .ok_or_else(|| AppError::Validation("条件来源没有结构化结果".into()))?;
            for segment in path.split('.') {
                actual = actual
                    .as_object()
                    .and_then(|object| object.get(segment))
                    .ok_or_else(|| AppError::Validation("条件结果字段不存在".into()))?;
            }
            Ok(match operator {
                EqualityOperator::Equal => actual == value,
                EqualityOperator::NotEqual => actual != value,
            })
        }
        WorkflowCondition::OutputContains { text, negated } => {
            let contains = context
                .output_summary
                .as_deref()
                .unwrap_or_default()
                .contains(text);
            Ok(if *negated { !contains } else { contains })
        }
    }
}

fn validate_result_path(path: &str) -> AppResult<()> {
    if path.is_empty()
        || path.len() > 512
        || path.split('.').any(|segment| {
            segment.is_empty()
                || segment.len() > 64
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
    {
        return Err(AppError::Validation("条件结果字段路径无效".into()));
    }
    Ok(())
}

fn compare_number(actual: i32, operator: NumericOperator, expected: i32) -> bool {
    match operator {
        NumericOperator::Equal => actual == expected,
        NumericOperator::NotEqual => actual != expected,
        NumericOperator::LessThan => actual < expected,
        NumericOperator::LessThanOrEqual => actual <= expected,
        NumericOperator::GreaterThan => actual > expected,
        NumericOperator::GreaterThanOrEqual => actual >= expected,
    }
}
