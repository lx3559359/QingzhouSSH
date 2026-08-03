use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    domain::execution::{now_millis, ExecutionFile, ExecutionStatus},
    error::{AppError, AppResult},
};

pub const MAX_EVENT_BYTES: usize = 32 * 1024;
pub const MAX_OUTPUT_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_SUMMARY_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionEvent {
    pub sequence: u64,
    pub emitted_at: i64,
    #[serde(flatten)]
    pub payload: ExecutionEventPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ExecutionEventPayload {
    Started {
        execution_id: Uuid,
        started_at: i64,
    },
    Stdout {
        text: String,
        total_bytes: u64,
    },
    Stderr {
        text: String,
        total_bytes: u64,
    },
    Progress {
        transferred: u64,
        total: Option<u64>,
        percent: Option<f64>,
    },
    FileProduced {
        file: ExecutionFile,
    },
    Finished {
        status: ExecutionStatus,
        exit_code: Option<i32>,
        duration_ms: u64,
        result: Option<Value>,
    },
    Failed {
        category: String,
        message: String,
        retryable: bool,
    },
}

#[derive(Debug, Default)]
pub struct EventSequence {
    current: u64,
}

impl EventSequence {
    pub fn next(&mut self, payload: ExecutionEventPayload) -> ExecutionEvent {
        self.current = self.current.saturating_add(1);
        ExecutionEvent {
            sequence: self.current,
            emitted_at: now_millis(),
            payload,
        }
    }
}

#[derive(Debug)]
pub struct OutputBudget {
    limit: u64,
    consumed: u64,
}

impl OutputBudget {
    pub fn new(limit: u64) -> Self {
        Self { limit, consumed: 0 }
    }

    pub fn consume(&mut self, bytes: usize) -> AppResult<u64> {
        let bytes = u64::try_from(bytes)
            .map_err(|_| AppError::Validation("输出片段大小超出范围".into()))?;
        let next = self
            .consumed
            .checked_add(bytes)
            .ok_or(AppError::OutputLimitExceeded { limit: self.limit })?;
        if next > self.limit {
            return Err(AppError::OutputLimitExceeded { limit: self.limit });
        }
        self.consumed = next;
        Ok(self.consumed)
    }

    pub fn consumed(&self) -> u64 {
        self.consumed
    }
}

#[derive(Debug)]
pub struct Utf8Chunker {
    max_bytes: usize,
    pending: Vec<u8>,
}

impl Utf8Chunker {
    pub fn new(max_bytes: usize) -> AppResult<Self> {
        if !(4..=MAX_EVENT_BYTES).contains(&max_bytes) {
            return Err(AppError::Validation(format!(
                "UTF-8 事件片段上限必须在 4 到 {MAX_EVENT_BYTES} 字节之间"
            )));
        }
        Ok(Self {
            max_bytes,
            pending: Vec::new(),
        })
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.pending.extend_from_slice(bytes);
        self.drain_decodable(false)
    }

    pub fn finish(&mut self) -> Vec<String> {
        self.drain_decodable(true)
    }

    fn drain_decodable(&mut self, finish: bool) -> Vec<String> {
        let mut text = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(value) => {
                    text.push_str(value);
                    self.pending.clear();
                    break;
                }
                Err(error) => {
                    let valid_up_to = error.valid_up_to();
                    if valid_up_to > 0 {
                        text.push_str(
                            std::str::from_utf8(&self.pending[..valid_up_to])
                                .expect("valid_up_to is guaranteed UTF-8"),
                        );
                        self.pending.drain(..valid_up_to);
                    }
                    match error.error_len() {
                        Some(length) => {
                            self.pending.drain(..length.min(self.pending.len()));
                            text.push('\u{FFFD}');
                        }
                        None if !finish => break,
                        None => {
                            self.pending.clear();
                            text.push('\u{FFFD}');
                            break;
                        }
                    }
                }
            }
        }
        split_utf8(&text, self.max_bytes)
    }
}

pub fn truncate_utf8(mut value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    value
}

fn split_utf8(value: &str, max_bytes: usize) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    for (index, character) in value.char_indices() {
        let end = index + character.len_utf8();
        if end - start > max_bytes {
            chunks.push(value[start..index].to_string());
            start = index;
        }
    }
    if start < value.len() {
        chunks.push(value[start..].to_string());
    }
    chunks
}
