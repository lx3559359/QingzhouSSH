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
    #[error("SSH 命令失败（退出码 {exit_status}）：{stderr}")]
    SshCommand { exit_status: i32, stderr: String },
    #[error("桌面窗口操作失败：{0}")]
    Tauri(#[from] tauri::Error),
}

impl AppError {
    pub fn ssh_command(exit_status: i32, mut stderr: String) -> Self {
        const MAX_STDERR_BYTES: usize = 8 * 1024;
        if stderr.len() > MAX_STDERR_BYTES {
            let mut boundary = MAX_STDERR_BYTES;
            while !stderr.is_char_boundary(boundary) {
                boundary -= 1;
            }
            stderr.truncate(boundary);
        }
        Self::SshCommand {
            exit_status,
            stderr,
        }
    }
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
            AppError::SshCommand { .. } => "ssh_command",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_command_error_caps_stderr_on_a_utf8_boundary() {
        let error = AppError::ssh_command(17, "错".repeat(4_000));
        let AppError::SshCommand {
            exit_status,
            stderr,
        } = error
        else {
            panic!("expected ssh command error");
        };
        assert_eq!(exit_status, 17);
        assert!(stderr.len() <= 8 * 1024);

        let serialized = serde_json::to_value(AppError::ssh_command(17, "failure".into())).unwrap();
        assert_eq!(serialized["code"], "ssh_command");
    }
}
