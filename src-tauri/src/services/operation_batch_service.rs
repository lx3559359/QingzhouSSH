use std::{collections::HashSet, future::Future, sync::Arc};

use tokio::{sync::Semaphore, task::JoinSet};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    core::{
        ssh::executor::VecEventSink,
        tasks::{
            built_in_catalog, task_version_is_compatible, validate_parameters, ExecutionScope,
            RiskLevel,
        },
    },
    domain::{
        operation::OperationStatus,
        operation_batch::{
            NewOperationBatch, OperationBatchDetails, OperationBatchItemStatus,
            OperationBatchRequest,
        },
    },
    error::{AppError, AppResult},
    repositories::{
        operation_batch_repository::OperationBatchRepository, server_repository::ServerRepository,
    },
    services::operation_service::{OperationService, OperationStartRequest},
};

const MAX_BATCH_SERVERS: usize = 20;
const BATCH_CONCURRENCY: usize = 3;

#[derive(Debug, Clone)]
pub struct BatchItemOutcome {
    pub operation_run_id: Option<Uuid>,
    pub status: OperationBatchItemStatus,
    pub error_message: Option<String>,
}

impl BatchItemOutcome {
    pub fn succeeded(operation_run_id: Option<Uuid>) -> Self {
        Self {
            operation_run_id,
            status: OperationBatchItemStatus::Succeeded,
            error_message: None,
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            operation_run_id: None,
            status: OperationBatchItemStatus::Failed,
            error_message: Some(message.into()),
        }
    }

    fn cancelled(operation_run_id: Option<Uuid>) -> Self {
        Self {
            operation_run_id,
            status: OperationBatchItemStatus::Cancelled,
            error_message: Some("用户取消批量任务".into()),
        }
    }
}

#[derive(Clone, Default)]
struct OperationBatchRegistry {
    tokens: Arc<tokio::sync::Mutex<std::collections::HashMap<Uuid, CancellationToken>>>,
}

impl OperationBatchRegistry {
    async fn register(&self, id: Uuid) -> AppResult<CancellationToken> {
        let mut tokens = self.tokens.lock().await;
        if tokens.contains_key(&id) {
            return Err(AppError::Validation("批量任务已经在运行".into()));
        }
        let token = CancellationToken::new();
        tokens.insert(id, token.clone());
        Ok(token)
    }

    async fn cancel(&self, id: Uuid) -> AppResult<()> {
        let tokens = self.tokens.lock().await;
        let token = tokens
            .get(&id)
            .ok_or_else(|| AppError::Validation("批量任务不存在或已经结束".into()))?;
        token.cancel();
        Ok(())
    }

    async fn remove(&self, id: Uuid) {
        self.tokens.lock().await.remove(&id);
    }
}

#[derive(Clone)]
pub struct OperationBatchService {
    repository: OperationBatchRepository,
    servers: ServerRepository,
    operations: OperationService,
    registry: OperationBatchRegistry,
}

impl OperationBatchService {
    pub fn new(
        repository: OperationBatchRepository,
        servers: ServerRepository,
        operations: OperationService,
    ) -> Self {
        Self {
            repository,
            servers,
            operations,
            registry: OperationBatchRegistry::default(),
        }
    }

    pub async fn start(&self, request: OperationBatchRequest) -> AppResult<OperationBatchDetails> {
        let operations = self.operations.clone();
        let child_request = request.clone();
        self.start_with_runner(request, move |server_id, cancel| {
            let operations = operations.clone();
            let request = child_request.clone();
            async move { run_operation(operations, server_id, request, cancel).await }
        })
        .await
    }

    pub async fn start_background(
        &self,
        request: OperationBatchRequest,
    ) -> AppResult<OperationBatchDetails> {
        let operations = self.operations.clone();
        let child_request = request.clone();
        self.start_background_with_runner(request, move |server_id, cancel| {
            let operations = operations.clone();
            let request = child_request.clone();
            async move { run_operation(operations, server_id, request, cancel).await }
        })
        .await
    }

    #[doc(hidden)]
    pub async fn start_with_runner<F, Fut>(
        &self,
        request: OperationBatchRequest,
        runner: F,
    ) -> AppResult<OperationBatchDetails>
    where
        F: Fn(String, CancellationToken) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = BatchItemOutcome> + Send + 'static,
    {
        let (batch_id, token) = self.prepare(&request).await?;
        self.run_prepared(batch_id, request.server_ids, token, runner)
            .await
    }

    #[doc(hidden)]
    pub async fn start_background_with_runner<F, Fut>(
        &self,
        request: OperationBatchRequest,
        runner: F,
    ) -> AppResult<OperationBatchDetails>
    where
        F: Fn(String, CancellationToken) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = BatchItemOutcome> + Send + 'static,
    {
        let (batch_id, token) = self.prepare(&request).await?;
        let service = self.clone();
        tokio::spawn(async move {
            let result = service
                .run_prepared(batch_id, request.server_ids, token, runner)
                .await;
            if let Err(error) = result {
                let _ = service
                    .repository
                    .fail_nonterminal_items(batch_id, &error.to_string())
                    .await;
                let _ = service.repository.complete(batch_id, false).await;
                service.registry.remove(batch_id).await;
            }
        });
        self.require_details(batch_id).await
    }

    pub async fn cancel(&self, id: Uuid) -> AppResult<()> {
        self.registry.cancel(id).await?;
        self.repository.cancel_queued_items(id).await?;
        Ok(())
    }

    pub async fn get(&self, id: Uuid) -> AppResult<Option<OperationBatchDetails>> {
        self.repository.get(id).await
    }

    async fn prepare(
        &self,
        request: &OperationBatchRequest,
    ) -> AppResult<(Uuid, CancellationToken)> {
        self.validate(request).await?;
        let batch = self
            .repository
            .create(NewOperationBatch {
                task_id: request.task_id.clone(),
                task_version: request.task_version,
                server_ids: request.server_ids.clone(),
            })
            .await?;
        let token = self.registry.register(batch.id).await?;
        Ok((batch.id, token))
    }

    async fn validate(&self, request: &OperationBatchRequest) -> AppResult<()> {
        if !(1..=MAX_BATCH_SERVERS).contains(&request.server_ids.len()) {
            return Err(AppError::Validation(
                "批量任务只能选择 1 到 20 台服务器".into(),
            ));
        }
        let mut unique = HashSet::with_capacity(request.server_ids.len());
        for server_id in &request.server_ids {
            if server_id.trim().is_empty()
                || server_id.contains('\0')
                || !unique.insert(server_id.as_str())
            {
                return Err(AppError::Validation("服务器列表包含空值或重复项".into()));
            }
        }
        let definition = built_in_catalog()
            .into_iter()
            .find(|definition| definition.id == request.task_id)
            .ok_or_else(|| AppError::Validation("运维任务不存在".into()))?;
        if !task_version_is_compatible(&definition, request.task_version) {
            return Err(AppError::Validation("运维任务版本不存在".into()));
        }
        if definition.risk_level != RiskLevel::Safe
            || definition.scope != ExecutionScope::ReadOnlyBatch
        {
            return Err(AppError::Validation(
                "批量执行仅允许标记为安全且支持批量的只读任务".into(),
            ));
        }
        validate_parameters(&definition, &request.parameters)?;
        for server_id in &request.server_ids {
            if self.servers.get(server_id).await?.is_none() {
                return Err(AppError::Validation(format!(
                    "所选服务器不存在：{server_id}"
                )));
            }
        }
        Ok(())
    }

    async fn run_prepared<F, Fut>(
        &self,
        batch_id: Uuid,
        server_ids: Vec<String>,
        token: CancellationToken,
        runner: F,
    ) -> AppResult<OperationBatchDetails>
    where
        F: Fn(String, CancellationToken) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = BatchItemOutcome> + Send + 'static,
    {
        let result = self
            .run_prepared_inner(batch_id, server_ids, token, runner)
            .await;
        self.registry.remove(batch_id).await;
        result
    }

    async fn run_prepared_inner<F, Fut>(
        &self,
        batch_id: Uuid,
        server_ids: Vec<String>,
        token: CancellationToken,
        runner: F,
    ) -> AppResult<OperationBatchDetails>
    where
        F: Fn(String, CancellationToken) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = BatchItemOutcome> + Send + 'static,
    {
        self.repository.mark_running(batch_id).await?;
        let semaphore = Arc::new(Semaphore::new(BATCH_CONCURRENCY));
        let mut jobs = JoinSet::new();
        for server_id in server_ids {
            let repository = self.repository.clone();
            let semaphore = semaphore.clone();
            let batch_cancel = token.clone();
            let runner = runner.clone();
            jobs.spawn(async move {
                let permit = tokio::select! {
                    _ = batch_cancel.cancelled() => {
                        return Ok::<(), AppError>(());
                    }
                    permit = semaphore.acquire_owned() => permit.map_err(|_| {
                        AppError::Validation("批量并发控制器已关闭".into())
                    })?,
                };
                if batch_cancel.is_cancelled() {
                    drop(permit);
                    return Ok(());
                }
                repository.mark_item_running(batch_id, &server_id).await?;
                let outcome = runner(server_id.clone(), batch_cancel.child_token()).await;
                repository
                    .finish_item(
                        batch_id,
                        &server_id,
                        outcome.status,
                        outcome.operation_run_id,
                        outcome.error_message,
                    )
                    .await?;
                drop(permit);
                Ok(())
            });
        }

        let mut worker_error = None;
        while let Some(result) = jobs.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    worker_error.get_or_insert(error);
                }
                Err(error) => {
                    worker_error.get_or_insert_with(|| {
                        AppError::Validation(format!("批量子任务异常结束：{error}"))
                    });
                }
            };
        }
        if token.is_cancelled() {
            self.repository.cancel_queued_items(batch_id).await?;
        }
        if let Some(error) = worker_error {
            self.repository
                .fail_nonterminal_items(batch_id, &error.to_string())
                .await?;
        }
        self.repository
            .complete(batch_id, token.is_cancelled())
            .await?;
        self.require_details(batch_id).await
    }

    async fn require_details(&self, id: Uuid) -> AppResult<OperationBatchDetails> {
        self.repository
            .get(id)
            .await?
            .ok_or_else(|| AppError::Validation("批量任务不存在".into()))
    }
}

async fn run_operation(
    operations: OperationService,
    server_id: String,
    request: OperationBatchRequest,
    cancel: CancellationToken,
) -> BatchItemOutcome {
    if cancel.is_cancelled() {
        return BatchItemOutcome::cancelled(None);
    }
    let mut events = VecEventSink::default();
    match operations
        .start_with_cancel(
            &server_id,
            OperationStartRequest {
                task_id: request.task_id,
                task_version: request.task_version,
                parameters: request.parameters,
                confirmed_preview_id: None,
            },
            &mut events,
            cancel,
        )
        .await
    {
        Ok(details) => match details.run.status {
            OperationStatus::Succeeded => BatchItemOutcome::succeeded(Some(details.run.id)),
            OperationStatus::Cancelled => BatchItemOutcome::cancelled(Some(details.run.id)),
            _ => BatchItemOutcome {
                operation_run_id: Some(details.run.id),
                status: OperationBatchItemStatus::Failed,
                error_message: details
                    .run
                    .error_message
                    .or_else(|| Some("服务器任务失败".into())),
            },
        },
        Err(AppError::Cancelled) => BatchItemOutcome::cancelled(None),
        Err(error) => BatchItemOutcome::failed(error.to_string()),
    }
}
