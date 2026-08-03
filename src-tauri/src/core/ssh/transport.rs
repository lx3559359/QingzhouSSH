use std::{
    io::{self, Read},
    net::{TcpStream, ToSocketAddrs},
    thread,
    time::{Duration, Instant},
};

use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
    Engine,
};
use serde::{Deserialize, Serialize};
use ssh2::{Channel, Session};

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
    timeout_millis(endpoint.timeout)?;
    Ok(())
}

fn timeout_millis(timeout: Duration) -> AppResult<u32> {
    if timeout.is_zero() {
        return Err(AppError::Validation("SSH 超时时间必须大于零".into()));
    }
    Ok(timeout.as_millis().clamp(1, u128::from(u32::MAX)) as u32)
}

fn open_session(endpoint: &SshEndpoint) -> AppResult<Session> {
    validate_endpoint(endpoint)?;
    let address = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| AppError::Validation("服务器地址无法解析".into()))?;
    let tcp = TcpStream::connect_timeout(&address, endpoint.timeout)?;
    tcp.set_read_timeout(Some(endpoint.timeout))?;
    tcp.set_write_timeout(Some(endpoint.timeout))?;

    let mut session = Session::new()?;
    session.set_tcp_stream(tcp);
    session.set_timeout(timeout_millis(endpoint.timeout)?);
    session.handshake()?;
    Ok(session)
}

fn observe_host_key(session: &Session) -> AppResult<HostKeyObservation> {
    let (raw, algorithm) = session
        .host_key()
        .ok_or_else(|| AppError::Security("服务器没有提供主机密钥".into()))?;

    Ok(HostKeyObservation {
        algorithm: format!("{algorithm:?}"),
        fingerprint_sha256: sha256_fingerprint(raw),
        raw_key_base64: STANDARD.encode(raw),
    })
}

pub fn inspect_host_key(endpoint: &SshEndpoint) -> AppResult<HostKeyObservation> {
    observe_host_key(&open_session(endpoint)?)
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

fn read_available<R: Read>(
    reader: &mut R,
    target: &mut Vec<u8>,
    other_len: usize,
) -> AppResult<bool> {
    let mut made_progress = false;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(made_progress),
            Ok(read) => {
                append_bounded(target, other_len, &buffer[..read])?;
                made_progress = true;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Ok(made_progress);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

fn read_channel_output(
    session: &Session,
    channel: &Channel,
    timeout: Duration,
) -> AppResult<(Vec<u8>, Vec<u8>)> {
    session.set_blocking(false);
    let result = (|| -> AppResult<(Vec<u8>, Vec<u8>)> {
        let mut stdout_stream = channel.stream(0);
        let mut stderr_stream = channel.stderr();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_millis(timeout)?));

        loop {
            let stdout_progress = read_available(&mut stdout_stream, &mut stdout, stderr.len())?;
            let stderr_progress = read_available(&mut stderr_stream, &mut stderr, stdout.len())?;
            if channel.eof() {
                break;
            }
            if Instant::now() >= deadline {
                return Err(
                    io::Error::new(io::ErrorKind::TimedOut, "SSH 命令执行或输出读取超时").into(),
                );
            }
            if !stdout_progress && !stderr_progress {
                thread::sleep(Duration::from_millis(2));
            }
        }
        Ok((stdout, stderr))
    })();
    session.set_blocking(true);
    result
}

pub fn execute(
    endpoint: &SshEndpoint,
    username: &str,
    credential: &StoredCredential,
    expected_fingerprint: &str,
    command: &str,
) -> AppResult<CommandOutput> {
    validate_execution_input(username, expected_fingerprint, command)?;
    let session = open_session(endpoint)?;
    let observed = observe_host_key(&session)?;
    if observed.fingerprint_sha256 != expected_fingerprint {
        return Err(AppError::Security(format!(
            "SSH 主机指纹已变化；期望 {expected_fingerprint}，实际 {}",
            observed.fingerprint_sha256
        )));
    }

    match credential {
        StoredCredential::Password { password } => {
            session.userauth_password(username, password)?;
        }
        StoredCredential::PrivateKey {
            private_key,
            passphrase,
        } => {
            session.userauth_pubkey_memory(username, None, private_key, passphrase.as_deref())?;
        }
    }
    if !session.authenticated() {
        return Err(AppError::Security("SSH 认证未成功".into()));
    }

    let mut channel = session.channel_session()?;
    channel.exec(command)?;
    let (stdout, stderr) = read_channel_output(&session, &channel, endpoint.timeout)?;
    channel.wait_close()?;
    let exit_status = channel.exit_status()?;

    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        exit_status,
    })
}

pub fn probe_system(
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
    )?;
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
    fn nonzero_submillisecond_timeout_never_becomes_infinite() {
        assert_eq!(timeout_millis(Duration::from_nanos(1)).unwrap(), 1);
    }

    #[test]
    fn rejects_combined_output_above_one_mebibyte() {
        let stdout = vec![b'a'; MAX_COMBINED_OUTPUT_BYTES];
        let mut stderr = Vec::new();
        assert!(append_bounded(&mut stderr, stdout.len(), b"x").is_err());
        assert!(stderr.is_empty());
    }
}
