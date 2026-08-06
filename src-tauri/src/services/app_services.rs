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
    domain::{
        execution::{ExecutionDetails, ExecutionFilter, ExecutionRecord},
        server::{CreateServerRequest, ServerProfile, StoredCredential, StoredHostKey},
    },
    error::{AppError, AppResult},
    repositories::{
        execution_repository::ExecutionRepository,
        operation_batch_repository::OperationBatchRepository,
        operation_repository::OperationRepository,
        operation_restore_repository::OperationRestoreRepository,
        script_repository::ScriptRepository, server_repository::ServerRepository,
        workflow_repository::WorkflowRepository,
    },
    services::{
        execution_service::{
            CustomExecutionRequest, ExecutionRegistry, ExecutionService, TaskAvailability,
            TaskExecutionRequest,
        },
        log_service::LogService,
        operation_batch_service::OperationBatchService,
        operation_report_service::OperationReportService,
        operation_restore_service::OperationRestoreService,
        operation_service::OperationService,
        remote_recovery_service::RemoteRecoveryService,
        restore_point_service::RestorePointService,
        script_service::ScriptService,
        server_connector::ServerConnector,
        task_remediation_service::TaskRemediationService,
        transfer_service::TransferService,
        workflow_diagnostics::WorkflowDiagnosticsService,
        workflow_nodes::{execution::ExecutionNodeAdapter, io::IoNodeAdapter},
        workflow_registry::WorkflowRunRegistry,
        workflow_service::WorkflowService,
    },
};

use crate::core::{
    logs::{LogResultPage, LogSearchRequest},
    sftp::{self, DirectoryListing, DownloadRequest, UploadRequest},
    ssh::executor::EventSink,
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
    executions: ExecutionService,
    task_remediation: TaskRemediationService,
    operations: OperationService,
    operation_batches: OperationBatchService,
    operation_reports: OperationReportService,
    operation_restore_points: OperationRestoreService,
    remote_recovery: RemoteRecoveryService,
    scripts: ScriptService,
    logs: LogService,
    transfers: TransferService,
    workflows: WorkflowRepository,
    restore_points: RestorePointService,
    workflow_runner: WorkflowService,
    workflow_diagnostics: WorkflowDiagnosticsService,
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
        let servers = ServerRepository::new(database.pool().clone());
        let vault = Vault::new(root, protector);
        let execution_repository = ExecutionRepository::new(database.pool().clone());
        execution_repository.recover_interrupted().await?;
        let operation_repository = OperationRepository::new(database.pool().clone());
        operation_repository.recover_interrupted().await?;
        let operation_restore_repository = OperationRestoreRepository::new(database.pool().clone());
        operation_restore_repository.recover_interrupted().await?;
        let operation_batch_repository = OperationBatchRepository::new(database.pool().clone());
        operation_batch_repository.recover_interrupted().await?;
        let workflow_repository = WorkflowRepository::new(database.pool().clone());
        workflow_repository.recover_interrupted().await?;
        let script_repository = ScriptRepository::new(database.pool().clone());
        let registry = ExecutionRegistry::default();
        let connector = ServerConnector::new(servers.clone(), vault.clone());
        let restore_points = RestorePointService::new(
            root.to_path_buf(),
            workflow_repository.clone(),
            connector.clone(),
        );
        let operation_restore_points = OperationRestoreService::new(
            root.to_path_buf(),
            operation_restore_repository,
            connector.clone(),
        );
        let executions = ExecutionService::new(
            root.to_path_buf(),
            execution_repository.clone(),
            connector.clone(),
            registry.clone(),
        );
        let remote_recovery = RemoteRecoveryService::new(connector.clone());
        let task_remediation = TaskRemediationService::new(connector.clone(), executions.clone());
        let operations = OperationService::new(
            operation_repository.clone(),
            executions.clone(),
            operation_restore_points.clone(),
            remote_recovery.clone(),
            connector.clone(),
        );
        let scripts = ScriptService::new(
            root.to_path_buf(),
            script_repository,
            operation_repository.clone(),
            executions.clone(),
        );
        let operation_batches = OperationBatchService::new(
            operation_batch_repository.clone(),
            servers.clone(),
            operations.clone(),
        );
        let operation_reports = OperationReportService::new(
            root.to_path_buf(),
            operation_repository,
            operation_batch_repository,
            connector.clone(),
        );
        let logs = LogService::new(
            root.to_path_buf(),
            execution_repository.clone(),
            connector.clone(),
            registry.clone(),
        );
        let transfers = TransferService::new(
            root.to_path_buf(),
            execution_repository,
            connector.clone(),
            registry,
        );
        let workflow_runner = WorkflowService::new(
            workflow_repository.clone(),
            ExecutionNodeAdapter::new(executions.clone()),
            IoNodeAdapter::new(logs.clone(), transfers.clone()),
            restore_points.clone(),
            connector,
            WorkflowRunRegistry::default(),
        );
        let workflow_diagnostics = WorkflowDiagnosticsService::new(
            root.to_path_buf(),
            workflow_repository.clone(),
            ServerConnector::new(servers.clone(), vault.clone()),
        );
        Ok(Self {
            data_root: root.to_path_buf(),
            servers,
            vault,
            executions,
            task_remediation,
            operations,
            operation_batches,
            operation_reports,
            operation_restore_points,
            remote_recovery,
            scripts,
            logs,
            transfers,
            workflows: workflow_repository,
            restore_points,
            workflow_runner,
            workflow_diagnostics,
        })
    }

    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    pub fn execution_service(&self) -> ExecutionService {
        self.executions.clone()
    }

    pub fn task_remediation_service(&self) -> TaskRemediationService {
        self.task_remediation.clone()
    }

    pub fn operation_service(&self) -> OperationService {
        self.operations.clone()
    }

    pub fn operation_batch_service(&self) -> OperationBatchService {
        self.operation_batches.clone()
    }

    pub fn operation_report_service(&self) -> OperationReportService {
        self.operation_reports.clone()
    }

    pub fn operation_restore_service(&self) -> OperationRestoreService {
        self.operation_restore_points.clone()
    }

    pub fn remote_recovery_service(&self) -> RemoteRecoveryService {
        self.remote_recovery.clone()
    }

    pub fn script_service(&self) -> ScriptService {
        self.scripts.clone()
    }

    pub fn log_service(&self) -> LogService {
        self.logs.clone()
    }

    pub fn transfer_service(&self) -> TransferService {
        self.transfers.clone()
    }

    pub fn workflow_repository(&self) -> WorkflowRepository {
        self.workflows.clone()
    }

    pub fn restore_point_service(&self) -> RestorePointService {
        self.restore_points.clone()
    }

    pub fn workflow_service(&self) -> WorkflowService {
        self.workflow_runner.clone()
    }

    pub fn workflow_diagnostics_service(&self) -> WorkflowDiagnosticsService {
        self.workflow_diagnostics.clone()
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

    pub async fn list_task_definitions(&self, server_id: &str) -> AppResult<Vec<TaskAvailability>> {
        self.executions.list_task_definitions(server_id).await
    }

    pub async fn start_task_execution<E: EventSink>(
        &self,
        server_id: &str,
        request: TaskExecutionRequest,
        events: &mut E,
    ) -> AppResult<ExecutionDetails> {
        self.executions
            .execute_task(server_id, request, events)
            .await
    }

    pub async fn start_custom_execution<E: EventSink>(
        &self,
        server_id: &str,
        request: CustomExecutionRequest,
        events: &mut E,
    ) -> AppResult<ExecutionDetails> {
        self.executions
            .execute_custom(server_id, request, events)
            .await
    }

    pub async fn cancel_execution(&self, execution_id: Uuid) -> AppResult<()> {
        self.executions.cancel(execution_id).await
    }

    pub async fn search_logs<E: EventSink>(
        &self,
        server_id: &str,
        request: LogSearchRequest,
        events: &mut E,
    ) -> AppResult<ExecutionDetails> {
        self.logs.search(server_id, request, events).await
    }

    pub async fn read_log_result_page(
        &self,
        execution_id: Uuid,
        cursor: Option<&str>,
        page_size: usize,
    ) -> AppResult<LogResultPage> {
        self.logs.read_page(execution_id, cursor, page_size).await
    }

    pub async fn download_log_result(
        &self,
        execution_id: Uuid,
        suggested_name: &str,
    ) -> AppResult<String> {
        self.logs
            .download_result(execution_id, suggested_name)
            .await
    }

    pub async fn list_local_directory(&self, path: Option<&Path>) -> AppResult<DirectoryListing> {
        sftp::list_local_directory(&self.data_root, path).await
    }

    pub async fn list_remote_directory(
        &self,
        server_id: &str,
        path: &str,
    ) -> AppResult<DirectoryListing> {
        let connector = ServerConnector::new(self.servers.clone(), self.vault.clone());
        let connected = connector.connect(server_id).await?;
        let listing = sftp::list_remote_directory(&connected.session, path).await;
        connected.session.disconnect().await;
        listing
    }

    pub async fn upload_file<E: EventSink>(
        &self,
        server_id: &str,
        request: UploadRequest,
        events: &mut E,
    ) -> AppResult<ExecutionDetails> {
        self.transfers.upload(server_id, request, events).await
    }

    pub async fn download_file<E: EventSink>(
        &self,
        server_id: &str,
        request: DownloadRequest,
        events: &mut E,
    ) -> AppResult<ExecutionDetails> {
        self.transfers.download(server_id, request, events).await
    }

    pub async fn list_executions(
        &self,
        filter: ExecutionFilter,
    ) -> AppResult<Vec<ExecutionRecord>> {
        self.executions.list(filter).await
    }

    pub async fn get_execution(&self, execution_id: Uuid) -> AppResult<Option<ExecutionDetails>> {
        self.executions.get(execution_id).await
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
