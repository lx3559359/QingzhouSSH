use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSearchRequest {
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
        let lower_path = self.path.to_ascii_lowercase();
        if !self.path.starts_with('/')
            || self.path.contains('\0')
            || self.path.split('/').any(|component| component == "..")
            || !(lower_path.ends_with(".log") || lower_path.ends_with(".gz"))
        {
            return Err(AppError::Validation(
                "日志路径必须是无 NUL 的 .log 或 .gz 绝对路径".into(),
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

    pub fn is_gzip(&self) -> bool {
        self.path.to_ascii_lowercase().ends_with(".gz")
    }
}

fn valid_time(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_digit() || matches!(byte, b'-' | b':' | b'.' | b'+' | b'T' | b'Z' | b' ')
        })
}
