use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::Mutex as AsyncMutex;

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
const SESSION_IDLE_TTL: Duration = Duration::from_secs(90);
const CAPABILITY_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Clone)]
pub struct ConnectedServer {
    pub profile: ServerProfile,
    pub session: AuthenticatedSshSession,
    pub capabilities: SystemCapabilities,
    pub redactor: Redactor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ConnectionIdentity {
    host: String,
    port: u16,
    username: String,
    auth_kind: String,
    credential_id: String,
    host_key_fingerprint: String,
}

impl ConnectionIdentity {
    fn new(profile: &ServerProfile, host_key_fingerprint: &str) -> Self {
        Self {
            host: profile.host.clone(),
            port: profile.port,
            username: profile.username.clone(),
            auth_kind: profile.auth_kind.as_str().into(),
            credential_id: profile.credential_id.clone(),
            host_key_fingerprint: host_key_fingerprint.into(),
        }
    }
}

struct CachedConnection {
    identity: ConnectionIdentity,
    connected: ConnectedServer,
    last_used_at: Instant,
    capabilities_checked_at: Instant,
}

#[derive(Default)]
struct ConnectionPool {
    entries: AsyncMutex<HashMap<String, CachedConnection>>,
    server_locks: AsyncMutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl ConnectionPool {
    async fn lock_for(&self, server_id: &str) -> Arc<AsyncMutex<()>> {
        let mut locks = self.server_locks.lock().await;
        locks
            .entry(server_id.into())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    async fn take(&self, server_id: &str) -> Option<CachedConnection> {
        self.entries.lock().await.remove(server_id)
    }

    async fn put(&self, server_id: &str, connection: CachedConnection) {
        self.entries
            .lock()
            .await
            .insert(server_id.into(), connection);
    }
}

fn may_reuse(session_open: bool, identity_matches: bool, idle_for: Duration) -> bool {
    session_open && identity_matches && idle_for < SESSION_IDLE_TTL
}

fn capabilities_are_fresh(age: Duration) -> bool {
    age < CAPABILITY_TTL
}

#[derive(Clone)]
pub struct ServerConnector {
    servers: ServerRepository,
    vault: Vault,
    pool: Arc<ConnectionPool>,
}

impl ServerConnector {
    pub fn new(servers: ServerRepository, vault: Vault) -> Self {
        Self {
            servers,
            vault,
            pool: Arc::new(ConnectionPool::default()),
        }
    }

    pub async fn connect(&self, server_id: &str) -> AppResult<ConnectedServer> {
        let server_lock = self.pool.lock_for(server_id).await;
        let _guard = server_lock.lock().await;
        let profile = self.require_server(server_id).await?;
        let fingerprint = self.trusted_fingerprint(&profile.id).await?;
        let identity = ConnectionIdentity::new(&profile, &fingerprint);
        let now = Instant::now();

        if let Some(mut cached) = self.pool.take(server_id).await {
            let can_reuse = may_reuse(
                !cached.connected.session.is_closed(),
                cached.identity == identity,
                now.duration_since(cached.last_used_at),
            );
            if can_reuse {
                if capabilities_are_fresh(now.duration_since(cached.capabilities_checked_at)) {
                    cached.last_used_at = now;
                    let connected = cached.connected.clone();
                    self.pool.put(server_id, cached).await;
                    return Ok(connected);
                }
                if let Ok(capabilities) = probe_authenticated(&cached.connected.session).await {
                    cached.connected.capabilities = capabilities;
                    cached.last_used_at = Instant::now();
                    cached.capabilities_checked_at = cached.last_used_at;
                    let connected = cached.connected.clone();
                    self.pool.put(server_id, cached).await;
                    return Ok(connected);
                }
            }
        }

        let host = profile.host.clone();
        let connected = self
            .connect_profile_at_host_with_fingerprint(profile, &host, &fingerprint)
            .await?;
        let connected_at = Instant::now();
        self.pool
            .put(
                server_id,
                CachedConnection {
                    identity,
                    connected: connected.clone(),
                    last_used_at: connected_at,
                    capabilities_checked_at: connected_at,
                },
            )
            .await;
        Ok(connected)
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
        let fingerprint = self.trusted_fingerprint(&profile.id).await?;
        self.connect_profile_at_host_with_fingerprint(profile, host, &fingerprint)
            .await
    }

    async fn connect_profile_at_host_with_fingerprint(
        &self,
        profile: ServerProfile,
        host: &str,
        fingerprint: &str,
    ) -> AppResult<ConnectedServer> {
        let encrypted_payload = self.vault.get(&profile.credential_id)?;
        let credential: StoredCredential = serde_json::from_slice(&encrypted_payload)
            .map_err(|_| AppError::Security("凭据密文损坏或格式无效".into()))?;
        let redactor = credential_redactor(&credential);
        let endpoint = SshEndpoint {
            host: host.into(),
            port: profile.port,
            timeout: DEFAULT_SSH_TIMEOUT,
        };
        let session =
            connect_authenticated(&endpoint, &profile.username, &credential, fingerprint).await?;
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
        self.servers.update_host(server_id, host).await?;
        self.invalidate(server_id).await;
        Ok(())
    }

    pub async fn invalidate(&self, server_id: &str) {
        let cached = self.pool.entries.lock().await.remove(server_id);
        if let Some(cached) = cached {
            cached.connected.session.disconnect().await;
        }
    }

    pub async fn shutdown(&self) {
        let entries = std::mem::take(&mut *self.pool.entries.lock().await);
        for (_, cached) in entries {
            cached.connected.session.disconnect().await;
        }
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

    async fn trusted_fingerprint(&self, server_id: &str) -> AppResult<String> {
        self.servers
            .get_host_key(server_id)
            .await?
            .map(|trusted| trusted.fingerprint_sha256)
            .ok_or_else(|| AppError::Security("尚未信任服务器主机密钥".into()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuses_only_matching_healthy_sessions_inside_idle_ttl() {
        let fresh = SESSION_IDLE_TTL - Duration::from_secs(1);
        assert!(may_reuse(true, true, fresh));
        assert!(!may_reuse(false, true, fresh));
        assert!(!may_reuse(true, false, fresh));
        assert!(!may_reuse(true, true, SESSION_IDLE_TTL));
    }

    #[test]
    fn refreshes_capabilities_after_their_ttl() {
        assert!(capabilities_are_fresh(
            CAPABILITY_TTL - Duration::from_secs(1)
        ));
        assert!(!capabilities_are_fresh(CAPABILITY_TTL));
    }
}
