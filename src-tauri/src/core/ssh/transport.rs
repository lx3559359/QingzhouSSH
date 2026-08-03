use std::{
    future::Future,
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
    Engine,
};
use russh::{
    client,
    keys::{self, PrivateKeyWithHashAlg, PublicKeyBase64},
    ChannelMsg, Disconnect,
};
use serde::{Deserialize, Serialize};

use crate::{
    core::{
        ssh::fingerprint::sha256_fingerprint,
        system_probe::{parse_probe, SystemCapabilities, PROBE_COMMAND},
    },
    domain::server::StoredCredential,
    error::{AppError, AppResult},
};

const MAX_COMBINED_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct SshEndpoint {
    pub host: String,
    pub port: u16,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyObservation {
    pub algorithm: String,
    pub fingerprint_sha256: String,
    pub raw_key_base64: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_status: i32,
}

struct HostKeyVerifier {
    expected_fingerprint: Option<String>,
    observation: Arc<Mutex<Option<HostKeyObservation>>>,
}

pub struct AuthenticatedSshSession {
    handle: client::Handle<HostKeyVerifier>,
    timeout: Duration,
}

impl AuthenticatedSshSession {
    pub async fn open_session_channel(&self) -> AppResult<russh::Channel<client::Msg>> {
        Ok(self.handle.channel_open_session().await?)
    }

    pub async fn disconnect(self) {
        let _ = self
            .handle
            .disconnect(Disconnect::ByApplication, "", "")
            .await;
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

impl client::Handler for HostKeyVerifier {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        let raw_key = server_public_key.public_key_bytes();
        let observed = HostKeyObservation {
            algorithm: server_public_key.algorithm().to_string(),
            fingerprint_sha256: sha256_fingerprint(&raw_key),
            raw_key_base64: STANDARD.encode(raw_key),
        };
        let trusted = self
            .expected_fingerprint
            .as_deref()
            .is_none_or(|expected| expected == observed.fingerprint_sha256);
        if let Ok(mut slot) = self.observation.lock() {
            *slot = Some(observed);
        } else {
            return Ok(false);
        }
        Ok(trusted)
    }
}

fn validate_endpoint(endpoint: &SshEndpoint) -> AppResult<()> {
    if endpoint.host.trim().is_empty() || endpoint.host.contains('\0') {
        return Err(AppError::Validation(
            "服务器地址不能为空或包含无效字符".into(),
        ));
    }
    if endpoint.port == 0 {
        return Err(AppError::Validation(
            "SSH 端口必须在 1 到 65535 之间".into(),
        ));
    }
    if endpoint.timeout.is_zero() {
        return Err(AppError::Validation("SSH 超时时间必须大于零".into()));
    }
    Ok(())
}

fn timeout_error(context: &str) -> AppError {
    io::Error::new(io::ErrorKind::TimedOut, format!("{context}超时")).into()
}

async fn with_timeout<T>(
    duration: Duration,
    context: &str,
    future: impl Future<Output = AppResult<T>>,
) -> AppResult<T> {
    tokio::time::timeout(duration, future)
        .await
        .map_err(|_| timeout_error(context))?
}

fn current_observation(
    observation: &Arc<Mutex<Option<HostKeyObservation>>>,
) -> AppResult<Option<HostKeyObservation>> {
    observation
        .lock()
        .map(|value| value.clone())
        .map_err(|_| AppError::Security("SSH 主机密钥检查状态异常".into()))
}

async fn connect_client(
    endpoint: &SshEndpoint,
    expected_fingerprint: Option<&str>,
) -> AppResult<(client::Handle<HostKeyVerifier>, HostKeyObservation)> {
    validate_endpoint(endpoint)?;
    let observation = Arc::new(Mutex::new(None));
    let handler = HostKeyVerifier {
        expected_fingerprint: expected_fingerprint.map(str::to_owned),
        observation: Arc::clone(&observation),
    };
    let config = Arc::new(client::Config {
        inactivity_timeout: Some(endpoint.timeout),
        ..Default::default()
    });
    let address = (endpoint.host.clone(), endpoint.port);
    let connection =
        tokio::time::timeout(endpoint.timeout, client::connect(config, address, handler))
            .await
            .map_err(|_| timeout_error("SSH 连接"))?;

    let session = match connection {
        Ok(session) => session,
        Err(error) => {
            if let (Some(expected), Some(observed)) =
                (expected_fingerprint, current_observation(&observation)?)
            {
                if expected != observed.fingerprint_sha256 {
                    return Err(AppError::Security(format!(
                        "SSH 主机指纹已变化；期望 {expected}，实际 {}",
                        observed.fingerprint_sha256
                    )));
                }
            }
            return Err(error.into());
        }
    };
    let observed = current_observation(&observation)?
        .ok_or_else(|| AppError::Security("服务器没有提供 SSH 主机密钥".into()))?;
    Ok((session, observed))
}

pub async fn inspect_host_key(endpoint: &SshEndpoint) -> AppResult<HostKeyObservation> {
    let (session, observed) = connect_client(endpoint, None).await?;
    let _ = session.disconnect(Disconnect::ByApplication, "", "").await;
    Ok(observed)
}

fn validate_execution_input(
    username: &str,
    expected_fingerprint: &str,
    command: &str,
) -> AppResult<()> {
    if username.trim().is_empty() || username.contains('\0') {
        return Err(AppError::Validation(
            "SSH 用户名不能为空或包含无效字符".into(),
        ));
    }
    if command.is_empty() || command.contains('\0') {
        return Err(AppError::Validation(
            "SSH 命令不能为空或包含无效字符".into(),
        ));
    }
    let encoded = expected_fingerprint
        .strip_prefix("SHA256:")
        .ok_or_else(|| AppError::Security("保存的主机指纹格式无效".into()))?;
    let digest = STANDARD_NO_PAD
        .decode(encoded)
        .map_err(|_| AppError::Security("保存的主机指纹格式无效".into()))?;
    if digest.len() != 32 {
        return Err(AppError::Security("保存的主机指纹格式无效".into()));
    }
    Ok(())
}

fn append_bounded(target: &mut Vec<u8>, other_len: usize, chunk: &[u8]) -> AppResult<()> {
    let combined = target
        .len()
        .checked_add(other_len)
        .and_then(|value| value.checked_add(chunk.len()))
        .ok_or_else(|| AppError::Validation("SSH 命令输出超过 1 MiB 上限".into()))?;
    if combined > MAX_COMBINED_OUTPUT_BYTES {
        return Err(AppError::Validation("SSH 命令输出超过 1 MiB 上限".into()));
    }
    target.extend_from_slice(chunk);
    Ok(())
}

async fn authenticate(
    session: &mut client::Handle<HostKeyVerifier>,
    username: &str,
    credential: &StoredCredential,
) -> AppResult<()> {
    let result = match credential {
        StoredCredential::Password { password } => {
            session
                .authenticate_password(username, password.as_str())
                .await?
        }
        StoredCredential::PrivateKey {
            private_key,
            passphrase,
        } => {
            let key = keys::decode_secret_key(private_key, passphrase.as_deref())
                .map_err(|_| AppError::Security("私钥格式或私钥口令无效".into()))?;
            let hash_algorithm = if matches!(key.algorithm(), keys::Algorithm::Rsa { .. }) {
                session.best_supported_rsa_hash().await?.flatten()
            } else {
                None
            };
            session
                .authenticate_publickey(
                    username,
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash_algorithm),
                )
                .await?
        }
    };
    if !result.success() {
        return Err(AppError::Security("SSH 认证未成功".into()));
    }
    Ok(())
}

async fn run_command(
    session: &client::Handle<HostKeyVerifier>,
    command: &str,
) -> AppResult<CommandOutput> {
    let mut channel = session.channel_open_session().await?;
    channel.exec(true, command).await?;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit_status = None;
    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Data { data } => {
                append_bounded(&mut stdout, stderr.len(), &data)?;
            }
            ChannelMsg::ExtendedData { data, .. } => {
                append_bounded(&mut stderr, stdout.len(), &data)?;
            }
            ChannelMsg::ExitStatus {
                exit_status: status,
            } => {
                exit_status = Some(i32::try_from(status).unwrap_or(i32::MAX));
            }
            _ => {}
        }
    }

    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        exit_status: exit_status.unwrap_or(-1),
    })
}

pub async fn execute(
    endpoint: &SshEndpoint,
    username: &str,
    credential: &StoredCredential,
    expected_fingerprint: &str,
    command: &str,
) -> AppResult<CommandOutput> {
    validate_execution_input(username, expected_fingerprint, command)?;
    let session =
        connect_authenticated(endpoint, username, credential, expected_fingerprint).await?;
    let output = with_timeout(endpoint.timeout, "SSH 命令执行或输出读取", async {
        run_command(&session.handle, command).await
    })
    .await;
    session.disconnect().await;
    output
}

pub async fn connect_authenticated(
    endpoint: &SshEndpoint,
    username: &str,
    credential: &StoredCredential,
    expected_fingerprint: &str,
) -> AppResult<AuthenticatedSshSession> {
    validate_execution_input(username, expected_fingerprint, "authenticated-session")?;
    let (mut session, observed) = connect_client(endpoint, Some(expected_fingerprint)).await?;
    if observed.fingerprint_sha256 != expected_fingerprint {
        return Err(AppError::Security(format!(
            "SSH 主机指纹已变化；期望 {expected_fingerprint}，实际 {}",
            observed.fingerprint_sha256
        )));
    }

    with_timeout(endpoint.timeout, "SSH 认证", async {
        authenticate(&mut session, username, credential).await
    })
    .await?;
    Ok(AuthenticatedSshSession {
        handle: session,
        timeout: endpoint.timeout,
    })
}

pub async fn probe_authenticated(
    session: &AuthenticatedSshSession,
) -> AppResult<SystemCapabilities> {
    let output = with_timeout(session.timeout(), "系统能力探测", async {
        run_command(&session.handle, PROBE_COMMAND).await
    })
    .await?;
    if output.exit_status != 0 {
        return Err(AppError::ssh_command(output.exit_status, output.stderr));
    }
    parse_probe(&output.stdout)
}

pub async fn probe_system(
    endpoint: &SshEndpoint,
    username: &str,
    credential: &StoredCredential,
    expected_fingerprint: &str,
) -> AppResult<SystemCapabilities> {
    let output = execute(
        endpoint,
        username,
        credential,
        expected_fingerprint,
        PROBE_COMMAND,
    )
    .await?;
    if output.exit_status != 0 {
        return Err(AppError::ssh_command(output.exit_status, output.stderr));
    }
    parse_probe(&output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_endpoint_before_network_access() {
        for endpoint in [
            SshEndpoint {
                host: " ".into(),
                port: 22,
                timeout: Duration::from_secs(5),
            },
            SshEndpoint {
                host: "example.test".into(),
                port: 0,
                timeout: Duration::from_secs(5),
            },
            SshEndpoint {
                host: "example.test".into(),
                port: 22,
                timeout: Duration::ZERO,
            },
        ] {
            assert!(validate_endpoint(&endpoint).is_err());
        }
    }

    #[test]
    fn rejects_combined_output_above_one_mebibyte() {
        let stdout = vec![b'a'; MAX_COMBINED_OUTPUT_BYTES];
        let mut stderr = Vec::new();
        assert!(append_bounded(&mut stderr, stdout.len(), b"x").is_err());
        assert!(stderr.is_empty());
    }
}
