use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter},
};
use uuid::Uuid;

use crate::{
    core::logs::parser::{LogLineKind, LogMatch},
    error::{AppError, AppResult},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredLogResults {
    pub count: usize,
    pub jsonl_relative_path: String,
    pub text_relative_path: String,
    pub jsonl_sha256: String,
    pub text_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogResultPage {
    pub items: Vec<LogMatch>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LogResultStore {
    data_root: PathBuf,
}

impl LogResultStore {
    pub fn new(data_root: &Path) -> Self {
        Self {
            data_root: data_root.to_path_buf(),
        }
    }

    pub async fn write(
        &self,
        execution_id: Uuid,
        matches: &[LogMatch],
    ) -> AppResult<StoredLogResults> {
        let directory = self.result_directory(execution_id);
        tokio::fs::create_dir_all(&directory).await?;
        let jsonl_path = directory.join("results.jsonl");
        let text_path = directory.join("results.txt");
        let mut jsonl = BufWriter::new(File::create(&jsonl_path).await?);
        let mut text = BufWriter::new(File::create(&text_path).await?);
        for item in matches {
            let encoded = serde_json::to_vec(item)
                .map_err(|error| AppError::Serialization(error.to_string()))?;
            jsonl.write_all(&encoded).await?;
            jsonl.write_all(b"\n").await?;
            let kind = match item.kind {
                LogLineKind::Match => "MATCH",
                LogLineKind::Context => "CONTEXT",
            };
            text.write_all(
                format!(
                    "{}:{} [{kind}] {}\n",
                    item.path, item.line_number, item.text
                )
                .as_bytes(),
            )
            .await?;
        }
        jsonl.flush().await?;
        text.flush().await?;
        drop(jsonl);
        drop(text);

        Ok(StoredLogResults {
            count: matches.len(),
            jsonl_relative_path: self.relative(&jsonl_path)?,
            text_relative_path: self.relative(&text_path)?,
            jsonl_sha256: sha256_file(&jsonl_path).await?,
            text_sha256: sha256_file(&text_path).await?,
        })
    }

    pub async fn read_page(
        &self,
        execution_id: Uuid,
        cursor: Option<&str>,
        page_size: usize,
    ) -> AppResult<LogResultPage> {
        if !(1..=200).contains(&page_size) {
            return Err(AppError::Validation(
                "日志结果每页数量必须在 1 到 200 之间".into(),
            ));
        }
        let offset = cursor
            .map(|value| {
                value
                    .parse::<usize>()
                    .map_err(|_| AppError::Validation("日志结果游标无效".into()))
            })
            .transpose()?
            .unwrap_or(0);
        let file = File::open(self.result_directory(execution_id).join("results.jsonl")).await?;
        let mut lines = BufReader::new(file).lines();
        let mut index = 0_usize;
        let mut items = Vec::with_capacity(page_size);
        let mut has_more = false;
        while let Some(line) = lines.next_line().await? {
            if index < offset {
                index += 1;
                continue;
            }
            if items.len() == page_size {
                has_more = true;
                break;
            }
            items.push(
                serde_json::from_str(&line)
                    .map_err(|error| AppError::Serialization(error.to_string()))?,
            );
            index += 1;
        }
        Ok(LogResultPage {
            next_cursor: has_more.then(|| (offset + items.len()).to_string()),
            items,
        })
    }

    pub fn text_path(&self, execution_id: Uuid) -> PathBuf {
        self.result_directory(execution_id).join("results.txt")
    }

    fn result_directory(&self, execution_id: Uuid) -> PathBuf {
        self.data_root
            .join("logs")
            .join("searches")
            .join(execution_id.to_string())
    }

    fn relative(&self, path: &Path) -> AppResult<String> {
        Ok(path
            .strip_prefix(&self.data_root)
            .map_err(|_| AppError::Security("日志结果路径逃逸数据根目录".into()))?
            .to_string_lossy()
            .replace('\\', "/"))
    }
}

async fn sha256_file(path: &Path) -> AppResult<String> {
    let mut file = File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
