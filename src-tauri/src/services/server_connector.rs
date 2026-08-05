use std::{net::IpAddr, time::Duration};

use crate::{
    core::{
        redaction::Redactor,
        ssh::transport::{
            connect_authenticated, probe_authenticated, AuthenticatedSshSession, SshEndpoint,
        },
        system_probe::SystemCapabilities,
        vault::Vault,
    },
    domain::server::{ServerProfile, StoredCredential},
    error::{AppError, AppResult},
    repositories::server_repository::ServerRepository,
};

const DEFAULT_SSH_TIMEOUT: Duration = Duration::from_secs(15);

pub struct ConnectedServer {
    pub profile: ServerProfile,
    pub session: AuthenticatedSshSession,
    pub capabilities: SystemCapabilities,
    pub redactor: Redactor,
}

#[derive(Clone)]
pub struct ServerConnector {
    servers: ServerRepository,
    vault: Vault,
}

impl ServerConnector {
    pub fn new(servers: ServerRepository, vault: Vault) -> Self {
        Self { servers, vault }
    }

    pub async fn connect(&self, server_id: &str) -> AppResult<ConnectedServer> {
        let profile = self.require_server(server_id).await?;
        let host = profile.host.clone();
        self.connect_profile_at_host(profile, &host).await
    }

    pub async fn connect_at_verified_ip(
        &self,
        server_id: &str,
        host: &str,
    ) -> AppResult<ConnectedServer> {
        host.parse::<IpAddr>()
            .map_err(|_| AppError::Validation("独立验证目标必须是有效 IP 地址".into()))?;
        let profile = self.require_server(server_id).await?;
        self.connect_profile_at_host(profile, host).await
    }

    async fn connect_profile_at_host(
        &self,
        profile: ServerProfile,
        host: &str,
    ) -> AppResult<ConnectedServer> {
        let trusted = self
            .servers
            .get_host_key(&profile.id)
            .await?
            .ok_or_else(|| AppError::Security("尚未信任服务器主机密钥".into()))?;
        let encrypted_payload = self.vault.get(&profile.credential_id)?;
        let credential: StoredCredential = serde_json::from_slice(&encrypted_payload)
            .map_err(|_| AppError::Security("凭据密文损坏或格式无效".into()))?;
        let redactor = credential_redactor(&credential);
        let endpoint = SshEndpoint {
            host: host.into(),
            port: profile.port,
            timeout: DEFAULT_SSH_TIMEOUT,
        };
        let session = connect_authenticated(
            &endpoint,
            &profile.username,
            &credential,
            &trusted.fingerprint_sha256,
        )
        .await?;
        let capabilities = match probe_authenticated(&session).await {
            Ok(capabilities) => capabilities,
            Err(error) => {
                session.disconnect().await;
                return Err(error);
            }
        };
        Ok(ConnectedServer {
            profile,
            session,
            capabilities,
            redactor,
        })
    }

    pub async fn commit_verified_host_change(&self, server_id: &str, host: &str) -> AppResult<()> {
        host.parse::<IpAddr>()
            .map_err(|_| AppError::Validation("已验证的新 IP 地址无效".into()))?;
        self.servers.update_host(server_id, host).await
    }

    pub async fn require_server(&self, server_id: &str) -> AppResult<ServerProfile> {
        if server_id.is_empty() {
            return Err(AppError::Validation("服务器标识不能为空".into()));
        }
        self.servers
            .get(server_id)
            .await?
            .ok_or_else(|| AppError::Validation("服务器不存在".into()))
    }

    pub async fn require_trusted_server(&self, server_id: &str) -> AppResult<ServerProfile> {
        let profile = self.require_server(server_id).await?;
        self.servers
            .get_host_key(server_id)
            .await?
            .ok_or_else(|| AppError::Security("尚未信任服务器主机密钥".into()))?;
        Ok(profile)
    }

    pub async fn redactor_for_server(&self, server_id: &str) -> AppResult<Redactor> {
        let profile = self.require_server(server_id).await?;
        let encrypted_payload = self.vault.get(&profile.credential_id)?;
        let credential: StoredCredential = serde_json::from_slice(&encrypted_payload)
            .map_err(|_| AppError::Security("凭据密文损坏或格式无效".into()))?;
        Ok(credential_redactor(&credential))
    }
}

fn credential_redactor(credential: &StoredCredential) -> Redactor {
    match credential {
        StoredCredential::Password { password } => Redactor::new([password.clone()]),
        StoredCredential::PrivateKey {
            private_key,
            passphrase,
        } => Redactor::new(std::iter::once(private_key.clone()).chain(passphrase.iter().cloned())),
    }
}
