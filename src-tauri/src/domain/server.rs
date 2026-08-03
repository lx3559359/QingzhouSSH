use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    Password,
    PrivateKey,
}

impl AuthKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::PrivateKey => "private_key",
        }
    }
}

impl TryFrom<&str> for AuthKind {
    type Error = AppError;

    fn try_from(value: &str) -> AppResult<Self> {
        match value {
            "password" => Ok(Self::Password),
            "private_key" => Ok(Self::PrivateKey),
            other => Err(AppError::Validation(format!("未知认证类型：{other}"))),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CredentialInput {
    Password {
        password: String,
    },
    PrivateKey {
        private_key: String,
        passphrase: Option<String>,
    },
}

impl CredentialInput {
    pub fn auth_kind(&self) -> AuthKind {
        match self {
            Self::Password { .. } => AuthKind::Password,
            Self::PrivateKey { .. } => AuthKind::PrivateKey,
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::Password { password } => password.is_empty(),
            Self::PrivateKey { private_key, .. } => private_key.is_empty(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateServerRequest {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub credential: CredentialInput,
}

impl CreateServerRequest {
    pub fn validate(&self) -> AppResult<()> {
        if self.name.trim().is_empty()
            || self.host.trim().is_empty()
            || self.username.trim().is_empty()
        {
            return Err(AppError::Validation("名称、地址和用户名不能为空".into()));
        }
        if self.name.len() > 128 || self.host.len() > 255 || self.username.len() > 128 {
            return Err(AppError::Validation("服务器字段超过长度限制".into()));
        }
        if self.name.contains('\0') || self.host.contains('\0') || self.username.contains('\0') {
            return Err(AppError::Validation("服务器字段包含无效字符".into()));
        }
        if self.port == 0 {
            return Err(AppError::Validation("端口必须在 1 到 65535 之间".into()));
        }
        if self.credential.is_empty() {
            return Err(AppError::Validation("认证凭据不能为空".into()));
        }
        match &self.credential {
            CredentialInput::Password { password } if password.len() > 16 * 1024 => {
                return Err(AppError::Validation("密码超过长度限制".into()));
            }
            CredentialInput::PrivateKey {
                private_key,
                passphrase,
            } if private_key.len() > 2 * 1024 * 1024
                || passphrase
                    .as_ref()
                    .is_some_and(|value| value.len() > 16 * 1024) =>
            {
                return Err(AppError::Validation("私钥或口令超过长度限制".into()));
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_kind: AuthKind,
    pub credential_id: String,
}

impl ServerProfile {
    pub fn new(
        name: &str,
        host: &str,
        port: u16,
        username: &str,
        auth_kind: AuthKind,
        credential_id: &str,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            host: host.into(),
            port,
            username: username.into(),
            auth_kind,
            credential_id: credential_id.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredHostKey {
    pub server_id: String,
    pub algorithm: String,
    pub fingerprint_sha256: String,
    pub raw_key_base64: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_host_and_zero_port() {
        let request = CreateServerRequest {
            name: "测试".into(),
            host: " ".into(),
            port: 0,
            username: "root".into(),
            credential: CredentialInput::Password {
                password: "secret".into(),
            },
        };
        assert!(request.validate().is_err());
    }
}
