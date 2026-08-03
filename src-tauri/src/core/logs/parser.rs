use serde::{Deserialize, Serialize};

use crate::{
    core::{logs::request::LogSearchRequest, redaction::Redactor},
    error::{AppError, AppResult},
};

const RECORD_PREFIX: &str = "__QZ_LOG__\u{1f}";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLineKind {
    Match,
    Context,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogMatch {
    pub path: String,
    pub line_number: u64,
    pub kind: LogLineKind,
    pub timestamp: Option<String>,
    pub text: String,
}

pub fn parse_search_output(
    request: &LogSearchRequest,
    output: &str,
    redactor: &Redactor,
) -> AppResult<Vec<LogMatch>> {
    request.validate()?;
    let mut results = Vec::new();
    let mut matched = 0_u32;
    for line in output.lines() {
        let Some(record) = line.strip_prefix(RECORD_PREFIX) else {
            continue;
        };
        let mut fields = record.splitn(3, '\u{1f}');
        let line_number = fields
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| AppError::Validation("日志检索输出行号无效".into()))?;
        let kind = match fields.next() {
            Some("match") => LogLineKind::Match,
            Some("context") => LogLineKind::Context,
            _ => return Err(AppError::Validation("日志检索输出类型无效".into())),
        };
        let text = fields
            .next()
            .ok_or_else(|| AppError::Validation("日志检索输出字段不完整".into()))?;
        let timestamp = extract_timestamp(text);
        if kind == LogLineKind::Match {
            if !within_time_range(timestamp.as_deref(), request) {
                continue;
            }
            if matched >= request.limit {
                continue;
            }
            matched += 1;
        }
        results.push(LogMatch {
            path: request.path.clone(),
            line_number,
            kind,
            timestamp,
            text: redactor.redact(text),
        });
    }
    Ok(results)
}

fn extract_timestamp(text: &str) -> Option<String> {
    let candidate = text.split_whitespace().next()?;
    if candidate.len() >= 10
        && candidate.as_bytes().get(4) == Some(&b'-')
        && candidate.as_bytes().get(7) == Some(&b'-')
        && candidate
            .bytes()
            .take(10)
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
    {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn within_time_range(timestamp: Option<&str>, request: &LogSearchRequest) -> bool {
    if request.start_time.is_none() && request.end_time.is_none() {
        return true;
    }
    let Some(timestamp) = timestamp else {
        return false;
    };
    request
        .start_time
        .as_deref()
        .is_none_or(|start| timestamp >= start)
        && request
            .end_time
            .as_deref()
            .is_none_or(|end| timestamp <= end)
}
