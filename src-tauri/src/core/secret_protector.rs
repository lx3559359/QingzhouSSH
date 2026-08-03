use crate::error::{AppError, AppResult};

pub trait SecretProtector: Send + Sync {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>>;
    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>>;
}

pub struct DpapiProtector;

impl SecretProtector for DpapiProtector {
    fn protect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        windows_dpapi::encrypt_data(value, windows_dpapi::Scope::User, None)
            .map_err(|error| AppError::Security(error.to_string()))
    }

    fn unprotect(&self, value: &[u8]) -> AppResult<Vec<u8>> {
        windows_dpapi::decrypt_data(value, windows_dpapi::Scope::User, None)
            .map_err(|error| AppError::Security(error.to_string()))
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn dpapi_user_scope_round_trips() {
        let protector = DpapiProtector;
        let encrypted = protector.protect(b"dpapi-canary").unwrap();
        assert_ne!(encrypted, b"dpapi-canary");
        assert_eq!(protector.unprotect(&encrypted).unwrap(), b"dpapi-canary");
    }
}
