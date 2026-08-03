use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use serde::Serialize;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    core::{
        data_root::initialize_data_root,
        database::Database,
        secret_protector::{DpapiProtector, SecretProtector},
        ssh::{
            transport::{self, HostKeyObservation, SshEndpoint},
            trust::{self, TrustDecision},
        },
        system_probe::SystemCapabilities,
        vault::Vault,
    },
    domain::server::{CreateServerRequest, ServerProfile, StoredCredential, StoredHostKey},
    error::{AppError, AppResult},
    repositories::server_repository::ServerRepository,
};

const DEFAULT_SSH_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyCheck {
    pub decision: TrustDecision,
    pub observed: HostKeyObservation,
    pub trusted: Option<StoredHostKey>,
}

#[derive(Clone)]
pub struct AppServices {
    data_root: PathBuf,
    servers: ServerRepository,
    vault: Vault,
}

impl AppServices {
    pub async fn open(root: &Path) -> AppResult<Self> {
        Self::open_with_protector(root, Arc::new(DpapiProtector)).await
    }

    pub async fn open_with_protector(
        root: &Path,
        protector: Arc<dyn SecretProtector>,
    ) -> AppResult<Self> {
        initialize_data_root(root)?;
        let database = Database::open(root).await?;
        Ok(Self {
            data_root: root.to_path_buf(),
            servers: ServerRepository::new(database.pool().clone()),
            vault: Vault::new(root, protector),
        })
    }

    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    pub async fn create_server(&self, request: CreateServerRequest) -> AppResult<ServerProfile> {
        request.validate()?;
        let CreateServerRequest {
            name,
            host,
            port,
            username,
            credential,
        } = request;
        let auth_kind = credential.auth_kind();
        let credential = StoredCredential::from(credential);
        let credential_id = Uuid::new_v4().to_string();
        let server = ServerProfile::new(&name, &host, port, &username, auth_kind, &credential_id);

        let encoded = Zeroizing::new(
            serde_json::to_vec(&credential)
                .map_err(|_| AppError::Security("无法安全序列化凭据".into()))?,
        );
        self.vault.put(&credential_id, &encoded)?;
        if let Err(error) = self.servers.insert(&server).await {
            self.vault.delete(&credential_id)?;
            return Err(error);
        }
        Ok(server)
    }

    pub async fn list_servers(&self) -> AppResult<Vec<ServerProfile>> {
        self.servers.list().await
    }

    pub async fn get_trusted_host_key(&self, server_id: &str) -> AppResult<Option<StoredHostKey>> {
        self.servers.get_host_key(server_id).await
    }

    pub async fn inspect_host_key(&self, server_id: &str) -> AppResult<HostKeyCheck> {
        let server = self.require_server(server_id).await?;
        let observed = inspect_endpoint(server_endpoint(&server)).await?;
        let trusted = self.servers.get_host_key(server_id).await?;
        let decision = trust::decide(
            trusted.as_ref().map(|key| key.fingerprint_sha256.as_str()),
            &observed.fingerprint_sha256,
        );
        Ok(HostKeyCheck {
            decision,
            observed,
            trusted,
        })
    }

    pub async fn trust_host_key(
        &self,
        server_id: &str,
        observation: HostKeyObservation,
    ) -> AppResult<()> {
        let server = self.require_server(server_id).await?;
        let fresh = inspect_endpoint(server_endpoint(&server)).await?;
        if fresh != observation {
            return Err(AppError::Security(
                "服务器主机密钥在确认过程中发生变化，已阻止信任".into(),
            ));
        }
        self.servers
            .upsert_host_key(&StoredHostKey {
                server_id: server.id,
                algorithm: fresh.algorithm,
                fingerprint_sha256: fresh.fingerprint_sha256,
                raw_key_base64: fresh.raw_key_base64,
            })
            .await
    }

    pub async fn test_connection(&self, server_id: &str) -> AppResult<SystemCapabilities> {
        let server = self.require_server(server_id).await?;
        let trusted = self
            .servers
            .get_host_key(server_id)
            .await?
            .ok_or_else(|| AppError::Security("尚未信任服务器主机密钥".into()))?;
        let encrypted_payload = self.vault.get(&server.credential_id)?;
        let credential: StoredCredential = serde_json::from_slice(&encrypted_payload)
            .map_err(|_| AppError::Security("凭据密文损坏或格式无效".into()))?;
        let endpoint = server_endpoint(&server);
        let username = server.username;
        let expected_fingerprint = trusted.fingerprint_sha256;

        transport::probe_system(&endpoint, &username, &credential, &expected_fingerprint).await
    }

    async fn require_server(&self, server_id: &str) -> AppResult<ServerProfile> {
        if server_id.is_empty() {
            return Err(AppError::Validation("服务器标识不能为空".into()));
        }
        self.servers
            .get(server_id)
            .await?
            .ok_or_else(|| AppError::Validation("服务器不存在".into()))
    }
}

fn server_endpoint(server: &ServerProfile) -> SshEndpoint {
    SshEndpoint {
        host: server.host.clone(),
        port: server.port,
        timeout: DEFAULT_SSH_TIMEOUT,
    }
}

async fn inspect_endpoint(endpoint: SshEndpoint) -> AppResult<HostKeyObservation> {
    transport::inspect_host_key(&endpoint).await
}
