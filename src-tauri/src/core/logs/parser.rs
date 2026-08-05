use serde::{Deserialize, Serialize};

use crate::{
    core::{
        logs::request::{LogSearchRequest, LogSearchTarget},
        redaction::Redactor,
    },
    error::{AppError, AppResult},
};

const RECORD_PREFIX: &str = "__QZ_LOG__\u{1f}";
const FILE_RECORD_PREFIX: &str = "__QZ_FILE__\u{1f}";

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileMatch {
    pub path: String,
    pub name: String,
    pub size: Option<u64>,
    pub modified_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "resultType", rename_all = "snake_case")]
pub enum SearchResultItem {
    Content(LogMatch),
    File(RemoteFileMatch),
}

pub fn parse_search_output(
    request: &LogSearchRequest,
    output: &str,
    redactor: &Redactor,
) -> AppResult<Vec<SearchResultItem>> {
    request.validate()?;
    match request.target {
        LogSearchTarget::Content => parse_content_output(request, output, redactor),
        LogSearchTarget::Filename => parse_filename_output(request, output),
    }
}

fn parse_content_output(
    request: &LogSearchRequest,
    output: &str,
    redactor: &Redactor,
) -> AppResult<Vec<SearchResultItem>> {
    request.validate()?;
    let mut results = Vec::new();
    let mut matched = 0_u32;
    for line in output.lines() {
        let Some(record) = line.strip_prefix(RECORD_PREFIX) else {
            continue;
        };
        let mut fields = record.splitn(4, '\u{1f}');
        let first = fields
            .next()
            .ok_or_else(|| AppError::Validation("日志检索输出字段不完整".into()))?;
        let (path, line_value) = if first.starts_with('/') {
            let line = fields
                .next()
                .ok_or_else(|| AppError::Validation("日志检索输出行号缺失".into()))?;
            (first.to_string(), line)
        } else {
            (request.path.clone(), first)
        };
        validate_result_path(&path)?;
        let line_number = line_value
            .parse::<u64>()
            .map_err(|_| AppError::Validation("日志检索输出行号无效".into()))?;
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
        results.push(SearchResultItem::Content(LogMatch {
            path,
            line_number,
            kind,
            timestamp,
            text: redactor.redact(text),
        }));
    }
    Ok(results)
}

fn parse_filename_output(
    request: &LogSearchRequest,
    output: &str,
) -> AppResult<Vec<SearchResultItem>> {
    let mut results = Vec::new();
    for line in output.lines() {
        let Some(record) = line.strip_prefix(FILE_RECORD_PREFIX) else {
            continue;
        };
        if results.len() >= request.limit as usize {
            break;
        }
        let mut fields = record.splitn(3, '\u{1f}');
        let path = fields
            .next()
            .ok_or_else(|| AppError::Validation("文件名查找输出路径缺失".into()))?;
        let size = parse_optional_u64(
            fields
                .next()
                .ok_or_else(|| AppError::Validation("文件名查找输出大小缺失".into()))?,
            "文件名查找输出大小无效",
        )?;
        let modified_at = parse_optional_u64(
            fields
                .next()
                .ok_or_else(|| AppError::Validation("文件名查找输出时间缺失".into()))?,
            "文件名查找输出时间无效",
        )?;
        validate_result_path(path)?;
        let name = path
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| AppError::Validation("文件名查找输出名称无效".into()))?;
        results.push(SearchResultItem::File(RemoteFileMatch {
            path: path.into(),
            name: name.into(),
            size,
            modified_at,
        }));
    }
    Ok(results)
}

fn parse_optional_u64(value: &str, message: &str) -> AppResult<Option<u64>> {
    if value.is_empty() {
        Ok(None)
    } else {
        value
            .parse()
            .map(Some)
            .map_err(|_| AppError::Validation(message.into()))
    }
}

fn validate_result_path(path: &str) -> AppResult<()> {
    if !path.starts_with('/')
        || path.ends_with('/')
        || path.split('/').any(|component| component == "..")
        || path
            .chars()
            .any(|character| matches!(character, '\0' | '\r' | '\n' | '\u{1f}'))
    {
        return Err(AppError::Validation("检索输出路径无效".into()));
    }
    Ok(())
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
