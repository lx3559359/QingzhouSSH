use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("输入无效：{0}")]
    Validation(String),
    #[error("应用尚未完成数据目录初始化")]
    NotReady,
    #[error("安全检查失败：{0}")]
    Security(String),
    #[error("I/O 操作失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("数据库操作失败：{0}")]
    Database(#[from] sqlx::Error),
    #[error("数据库迁移失败：{0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("SSH 操作失败：{0}")]
    Ssh(#[from] ssh2::Error),
    #[error("桌面窗口操作失败：{0}")]
    Tauri(#[from] tauri::Error),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct Payload<'a> {
            code: &'a str,
            message: String,
        }

        let code = match self {
            AppError::Validation(_) => "validation",
            AppError::NotReady => "not_ready",
            AppError::Security(_) => "security",
            AppError::Io(_) => "io",
            AppError::Database(_) => "database",
            AppError::Migration(_) => "migration",
            AppError::Ssh(_) => "ssh",
            AppError::Tauri(_) => "tauri",
        };
        Payload {
            code,
            message: self.to_string(),
        }
        .serialize(serializer)
    }
}

pub type AppResult<T> = Result<T, AppError>;
