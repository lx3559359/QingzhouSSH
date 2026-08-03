use std::{io, path::Path, time::Duration};

use russh::ChannelMsg;
use serde::{Deserialize, Serialize};
use tokio::{fs::File, io::AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    core::{redaction::Redactor, ssh::transport::AuthenticatedSshSession},
    domain::events::{
        EventSequence, ExecutionEvent, ExecutionEventPayload, OutputBudget, Utf8Chunker,
        MAX_EVENT_BYTES, MAX_OUTPUT_BYTES,
    },
    error::{AppError, AppResult},
};

#[derive(Debug, Clone)]
pub struct CommandRequest {
    pub execution_id: Uuid,
    pub command: String,
    pub timeout: Duration,
    pub max_output_bytes: u64,
}

impl CommandRequest {
    pub fn validate(&self) -> AppResult<()> {
        if self.command.is_empty() || self.command.contains('\0') {
            return Err(AppError::Validation("SSH 命令不能为空或包含 NUL".into()));
        }
        if self.timeout.is_zero() {
            return Err(AppError::Validation("SSH 命令超时时间必须大于零".into()));
        }
        if self.max_output_bytes == 0 || self.max_output_bytes > MAX_OUTPUT_BYTES {
            return Err(AppError::Validation(format!(
                "SSH 输出上限必须在 1 到 {MAX_OUTPUT_BYTES} 字节之间"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandOutcome {
    pub exit_status: i32,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
}

pub trait EventSink: Send {
    fn send(&mut self, event: ExecutionEvent) -> AppResult<()>;
}

#[derive(Debug, Default)]
pub struct VecEventSink {
    pub events: Vec<ExecutionEvent>,
}

impl EventSink for VecEventSink {
    fn send(&mut self, event: ExecutionEvent) -> AppResult<()> {
        self.events.push(event);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

pub struct StreamEventWriter<'a, E: EventSink> {
    file: File,
    redactor: &'a Redactor,
    sink: &'a mut E,
    sequence: EventSequence,
    budget: OutputBudget,
    event_bytes: usize,
    stdout_decoder: Utf8Chunker,
    stderr_decoder: Utf8Chunker,
    stdout_pending: String,
    stderr_pending: String,
    stdout_bytes: u64,
    stderr_bytes: u64,
}

impl<'a, E: EventSink> StreamEventWriter<'a, E> {
    pub async fn open(
        output_path: &Path,
        redactor: &'a Redactor,
        sink: &'a mut E,
        max_output_bytes: u64,
        event_bytes: usize,
    ) -> AppResult<Self> {
        if max_output_bytes == 0 || max_output_bytes > MAX_OUTPUT_BYTES {
            return Err(AppError::Validation("命令输出上限无效".into()));
        }
        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        Ok(Self {
            file: File::create(output_path).await?,
            redactor,
            sink,
            sequence: EventSequence::default(),
            budget: OutputBudget::new(max_output_bytes),
            event_bytes,
            stdout_decoder: Utf8Chunker::new(MAX_EVENT_BYTES)?,
            stderr_decoder: Utf8Chunker::new(MAX_EVENT_BYTES)?,
            stdout_pending: String::new(),
            stderr_pending: String::new(),
            stdout_bytes: 0,
            stderr_bytes: 0,
        })
    }

    pub fn emit(&mut self, payload: ExecutionEventPayload) -> AppResult<()> {
        self.sink.send(self.sequence.next(payload))
    }

    pub async fn write(&mut self, stream: OutputStream, bytes: &[u8]) -> AppResult<()> {
        let total = self.budget.consume(bytes.len())?;
        match stream {
            OutputStream::Stdout => {
                self.stdout_bytes = self.stdout_bytes.saturating_add(bytes.len() as u64);
                let chunks = self.stdout_decoder.push(bytes);
                self.stdout_pending.extend(chunks);
            }
            OutputStream::Stderr => {
                self.stderr_bytes = self.stderr_bytes.saturating_add(bytes.len() as u64);
                let chunks = self.stderr_decoder.push(bytes);
                self.stderr_pending.extend(chunks);
            }
        }
        self.flush_complete_lines(stream, total).await
    }

    pub async fn finish(mut self) -> AppResult<CommandStreamTotals> {
        self.stdout_pending.extend(self.stdout_decoder.finish());
        self.stderr_pending.extend(self.stderr_decoder.finish());
        let stdout = std::mem::take(&mut self.stdout_pending);
        let stderr = std::mem::take(&mut self.stderr_pending);
        self.emit_text(OutputStream::Stdout, &stdout).await?;
        self.emit_text(OutputStream::Stderr, &stderr).await?;
        self.file.flush().await?;
        Ok(CommandStreamTotals {
            stdout_bytes: self.stdout_bytes,
            stderr_bytes: self.stderr_bytes,
        })
    }

    async fn flush_complete_lines(&mut self, stream: OutputStream, _total: u64) -> AppResult<()> {
        let pending = match stream {
            OutputStream::Stdout => &mut self.stdout_pending,
            OutputStream::Stderr => &mut self.stderr_pending,
        };
        let Some(last_newline) = pending.rfind('\n') else {
            return Ok(());
        };
        let text = pending[..=last_newline].to_string();
        pending.drain(..=last_newline);
        self.emit_text(stream, &text).await
    }

    async fn emit_text(&mut self, stream: OutputStream, text: &str) -> AppResult<()> {
        if text.is_empty() {
            return Ok(());
        }
        let redacted = self.redactor.redact(text);
        let mut chunker = Utf8Chunker::new(self.event_bytes)?;
        let mut chunks = chunker.push(redacted.as_bytes());
        chunks.extend(chunker.finish());
        for chunk in chunks {
            let (prefix, payload) = match stream {
                OutputStream::Stdout => (
                    b"[stdout] ".as_slice(),
                    ExecutionEventPayload::Stdout {
                        text: chunk.clone(),
                        total_bytes: self.stdout_bytes,
                    },
                ),
                OutputStream::Stderr => (
                    b"[stderr] ".as_slice(),
                    ExecutionEventPayload::Stderr {
                        text: chunk.clone(),
                        total_bytes: self.stderr_bytes,
                    },
                ),
            };
            self.file.write_all(prefix).await?;
            self.file.write_all(chunk.as_bytes()).await?;
            self.file.write_all(b"\n").await?;
            self.emit(payload)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandStreamTotals {
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
}

pub async fn execute_streaming<E: EventSink>(
    session: &AuthenticatedSshSession,
    request: CommandRequest,
    output_file: &Path,
    redactor: &Redactor,
    events: &mut E,
    cancel: CancellationToken,
) -> AppResult<CommandOutcome> {
    request.validate()?;
    let started_at = crate::domain::execution::now_millis();
    let mut channel = session.open_session_channel().await?;
    channel.exec(true, request.command.as_str()).await?;
    let mut writer = StreamEventWriter::open(
        output_file,
        redactor,
        events,
        request.max_output_bytes,
        MAX_EVENT_BYTES,
    )
    .await?;
    writer.emit(ExecutionEventPayload::Started {
        execution_id: request.execution_id,
        started_at,
    })?;

    let mut exit_status = None;
    let read_result = tokio::time::timeout(request.timeout, async {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    return Err(AppError::Cancelled);
                }
                message = channel.wait() => {
                    let Some(message) = message else { break };
                    match message {
                        ChannelMsg::Data { data } => writer.write(OutputStream::Stdout, &data).await?,
                        ChannelMsg::ExtendedData { data, .. } => writer.write(OutputStream::Stderr, &data).await?,
                        ChannelMsg::ExitStatus { exit_status: status } => {
                            exit_status = Some(i32::try_from(status).unwrap_or(i32::MAX));
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok::<(), AppError>(())
    })
    .await;

    match read_result {
        Err(_) => {
            let _ = channel.close().await;
            return Err(
                io::Error::new(io::ErrorKind::TimedOut, "SSH 命令执行或输出读取超时").into(),
            );
        }
        Ok(Err(AppError::Cancelled)) => {
            return if channel.close().await.is_ok() {
                Err(AppError::Cancelled)
            } else {
                Err(AppError::RemoteStateUncertain(
                    "取消时无法确认远程 channel 已关闭".into(),
                ))
            };
        }
        Ok(Err(error)) => return Err(error),
        Ok(Ok(())) => {}
    }

    let totals = writer.finish().await?;
    Ok(CommandOutcome {
        exit_status: exit_status.unwrap_or(-1),
        stdout_bytes: totals.stdout_bytes,
        stderr_bytes: totals.stderr_bytes,
    })
}
