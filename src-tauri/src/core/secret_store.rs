use zeroize::Zeroizing;

use crate::error::{AppError, AppResult};

pub trait SecretStore: Send + Sync {
    fn put(&self, id: &str, secret: &[u8]) -> AppResult<()>;
    fn get(&self, id: &str) -> AppResult<Zeroizing<Vec<u8>>>;
    fn delete(&self, id: &str) -> AppResult<()>;
}

pub fn validate_secret_id(id: &str) -> AppResult<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(AppError::Validation("凭据标识格式无效".into()));
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeKeyringSecretStore;

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl NativeKeyringSecretStore {
    fn entry(id: &str) -> AppResult<keyring::Entry> {
        validate_secret_id(id)?;
        keyring::Entry::new("com.qingzhoussh.desktop", id)
            .map_err(|error| native_store_error("打开", error))
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl SecretStore for NativeKeyringSecretStore {
    fn put(&self, id: &str, secret: &[u8]) -> AppResult<()> {
        let entry = Self::entry(id)?;
        match entry.get_secret() {
            Ok(existing) => {
                drop(Zeroizing::new(existing));
                return Err(AppError::Validation("凭据标识已经存在".into()));
            }
            Err(keyring::Error::NoEntry) => {}
            Err(error) => return Err(native_store_error("检查", error)),
        }
        entry
            .set_secret(secret)
            .map_err(|error| native_store_error("写入", error))
    }

    fn get(&self, id: &str) -> AppResult<Zeroizing<Vec<u8>>> {
        Ok(Zeroizing::new(
            Self::entry(id)?
                .get_secret()
                .map_err(|error| native_store_error("读取", error))?,
        ))
    }

    fn delete(&self, id: &str) -> AppResult<()> {
        let entry = Self::entry(id)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(native_store_error("删除", error)),
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn native_store_error(action: &str, error: keyring::Error) -> AppError {
    AppError::Security(format!(
        "无法{action}系统安全存储中的凭据；请确认 Keychain 或 Secret Service 已解锁且可用：{error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_ids_are_bounded_and_path_safe() {
        assert!(validate_secret_id("cred-123").is_ok());
        assert!(validate_secret_id("").is_err());
        assert!(validate_secret_id("../credential").is_err());
        assert!(validate_secret_id(&"x".repeat(129)).is_err());
    }
}
