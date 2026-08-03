use std::{
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use ssh2::Session;

use crate::{
    core::ssh::fingerprint::sha256_fingerprint,
    error::{AppError, AppResult},
};

#[derive(Debug, Clone)]
pub struct SshEndpoint {
    pub host: String,
    pub port: u16,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyObservation {
    pub algorithm: String,
    pub fingerprint_sha256: String,
    pub raw_key_base64: String,
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

pub fn inspect_host_key(endpoint: &SshEndpoint) -> AppResult<HostKeyObservation> {
    let session = open_session(endpoint)?;
    let (raw, algorithm) = session
        .host_key()
        .ok_or_else(|| AppError::Security("服务器没有提供主机密钥".into()))?;

    Ok(HostKeyObservation {
        algorithm: format!("{algorithm:?}"),
        fingerprint_sha256: sha256_fingerprint(raw),
        raw_key_base64: STANDARD.encode(raw),
    })
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
}
