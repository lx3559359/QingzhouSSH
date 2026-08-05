use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogSearchTarget {
    #[default]
    Content,
    Filename,
}

impl LogSearchTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Filename => "filename",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSearchRequest {
    #[serde(default)]
    pub target: LogSearchTarget,
    pub path: String,
    pub keyword: String,
    pub case_sensitive: bool,
    pub context_lines: u8,
    pub limit: u32,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}

impl LogSearchRequest {
    pub fn validate(&self) -> AppResult<()> {
        if self.target == LogSearchTarget::Filename {
            return self.validate_filename_search();
        }
        if !self.path.is_empty()
            && (!self.path.starts_with('/')
                || self.path.contains('\0')
                || self.path.split('/').any(|component| component == "..")
                || self.path.ends_with('/'))
        {
            return Err(AppError::Validation(
                "指定日志路径必须是远程文件的绝对路径".into(),
            ));
        }
        if self.keyword.is_empty() || self.keyword.contains('\0') || self.keyword.len() > 512 {
            return Err(AppError::Validation("日志关键词无效或超过 512 字节".into()));
        }
        if self.context_lines > 20 {
            return Err(AppError::Validation("日志上下文不能超过 20 行".into()));
        }
        if !(1..=10_000).contains(&self.limit) {
            return Err(AppError::Validation(
                "日志结果上限必须在 1 到 10000 之间".into(),
            ));
        }
        if self
            .start_time
            .as_deref()
            .is_some_and(|value| !valid_time(value))
            || self
                .end_time
                .as_deref()
                .is_some_and(|value| !valid_time(value))
        {
            return Err(AppError::Validation("日志时间范围格式无效".into()));
        }
        if let (Some(start), Some(end)) = (&self.start_time, &self.end_time) {
            if start > end {
                return Err(AppError::Validation("日志开始时间不能晚于结束时间".into()));
            }
        }
        Ok(())
    }

    fn validate_filename_search(&self) -> AppResult<()> {
        if !self.path.is_empty() {
            return Err(AppError::Validation(
                "按文件名查找时不能指定日志路径".into(),
            ));
        }
        if self.keyword.is_empty()
            || self.keyword.len() > 256
            || self
                .keyword
                .chars()
                .any(|character| matches!(character, '\0' | '\r' | '\n' | '\u{1f}'))
        {
            return Err(AppError::Validation(
                "文件名关键字必须为 1 到 256 字节，且不能包含控制字符".into(),
            ));
        }
        if self.case_sensitive
            || self.context_lines != 0
            || !(1..=200).contains(&self.limit)
            || self.start_time.is_some()
            || self.end_time.is_some()
        {
            return Err(AppError::Validation(
                "文件名查找参数无效：仅支持忽略大小写、最多 200 个结果".into(),
            ));
        }
        Ok(())
    }

    pub fn is_gzip(&self) -> bool {
        self.path.to_ascii_lowercase().ends_with(".gz")
    }

    pub fn is_smart_search(&self) -> bool {
        self.target == LogSearchTarget::Content && self.path.is_empty()
    }
}

fn valid_time(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_digit() || matches!(byte, b'-' | b':' | b'.' | b'+' | b'T' | b'Z' | b' ')
        })
}
