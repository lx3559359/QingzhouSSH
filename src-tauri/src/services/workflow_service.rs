use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    core::{
        redaction::Redactor,
        ssh::executor::{EventSink, VecEventSink},
        tasks::{built_in_catalog, select_implementation, task_version_is_compatible, RiskLevel},
        workflows::{evaluate_condition, require_valid_workflow, ConditionContext},
    },
    domain::{
        events::{ExecutionEvent, ExecutionEventPayload},
        execution::now_millis,
        workflow::{
            FinishWorkflowNode, FinishWorkflowRun, NewWorkflowRun, WorkflowDefinition,
            WorkflowDraft, WorkflowEdgeBranch, WorkflowNodeConfig, WorkflowNodeRun,
            WorkflowNodeStatus, WorkflowRunDetails, WorkflowRunStatus,
        },
        workflow_events::{WorkflowEvent, WorkflowEventPayload, WorkflowEventSink},
    },
    error::{AppError, AppResult},
    repositories::workflow_repository::WorkflowRepository,
    services::{
        restore_point_service::RestorePointService,
        server_connector::ServerConnector,
        workflow_nodes::{execution::ExecutionNodeAdapter, io::IoNodeAdapter, NodeOutcome},
        workflow_registry::WorkflowRunRegistry,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartWorkflowRunRequest {
    pub workflow_id: Uuid,
    pub workflow_version: Option<i32>,
    pub server_id: String,
    #[serde(default)]
    pub dangerous_confirmed: bool,
}

#[derive(Clone)]
pub struct WorkflowService {
    workflows: WorkflowRepository,
    execution_nodes: ExecutionNodeAdapter,
    io_nodes: IoNodeAdapter,
    restore_points: RestorePointService,
    connector: ServerConnector,
    registry: WorkflowRunRegistry,
}

struct StepOutcome {
    status: WorkflowNodeStatus,
    execution_id: Option<Uuid>,
    exit_code: Option<i32>,
    result: Option<Value>,
    output_summary: Option<String>,
    error_category: Option<String>,
    error_message: Option<String>,
    retryable: bool,
    condition_result: Option<bool>,
}

struct NodeExecutionContext<'a, S> {
    run_id: Uuid,
    node_id: Uuid,
    server_id: &'a str,
    dangerous_confirmed: bool,
    contexts: &'a HashMap<Uuid, ConditionContext>,
    cancel: CancellationToken,
    events: &'a mut S,
}

struct RunContinuation<'a, S> {
    definition: &'a WorkflowDefinition,
    run_id: Uuid,
    server_id: &'a str,
    dangerous_confirmed: bool,
    current: Uuid,
    contexts: HashMap<Uuid, ConditionContext>,
    visited: HashSet<Uuid>,
    cancel: CancellationToken,
    events: &'a mut S,
}

struct ChildTrackingSink {
    inner: VecEventSink,
    registry: WorkflowRunRegistry,
    run_id: Uuid,
    child: Option<Uuid>,
}

impl ChildTrackingSink {
    fn new(registry: WorkflowRunRegistry, run_id: Uuid) -> Self {
        Self {
            inner: VecEventSink::default(),
            registry,
            run_id,
            child: None,
        }
    }

    fn finish(&mut self) -> AppResult<()> {
        if let Some(child) = self.child.take() {
            self.registry.clear_child_now(self.run_id, child)?;
        }
        Ok(())
    }
}

impl EventSink for ChildTrackingSink {
    fn send(&mut self, event: ExecutionEvent) -> AppResult<()> {
        match &event.payload {
            ExecutionEventPayload::Started { execution_id, .. } => {
                self.registry.set_child_now(self.run_id, *execution_id)?;
                self.child = Some(*execution_id);
            }
            ExecutionEventPayload::Finished { .. } | ExecutionEventPayload::Failed { .. } => {
                self.finish()?;
            }
            _ => {}
        }
        self.inner.send(event)
    }
}

impl StepOutcome {
    fn succeeded(output_summary: impl Into<String>, result: Option<Value>) -> Self {
        Self {
            status: WorkflowNodeStatus::Succeeded,
            execution_id: None,
            exit_code: Some(0),
            result,
            output_summary: Some(output_summary.into()),
            error_category: None,
            error_message: None,
            retryable: false,
            condition_result: None,
        }
    }

    fn failed(error: AppError) -> Self {
        Self {
            status: WorkflowNodeStatus::Failed,
            execution_id: None,
            exit_code: None,
            result: None,
            output_summary: None,
            error_category: Some(error.code().into()),
            error_message: Some(error.to_string()),
            retryable: error.retryable(),
            condition_result: None,
        }
    }
}

impl From<NodeOutcome> for StepOutcome {
    fn from(outcome: NodeOutcome) -> Self {
        Self {
            status: outcome.status,
            execution_id: Some(outcome.execution_id),
            exit_code: outcome.exit_code,
            result: outcome.result,
            output_summary: outcome.output_summary,
            error_category: outcome.error_category,
            error_message: outcome.error_message,
            retryable: outcome.retryable,
            condition_result: None,
        }
    }
}

impl WorkflowService {
    pub fn new(
        workflows: WorkflowRepository,
        execution_nodes: ExecutionNodeAdapter,
        io_nodes: IoNodeAdapter,
        restore_points: RestorePointService,
        connector: ServerConnector,
        registry: WorkflowRunRegistry,
    ) -> Self {
        Self {
            workflows,
            execution_nodes,
            io_nodes,
            restore_points,
            connector,
            registry,
        }
    }

    pub async fn run<S: WorkflowEventSink>(
        &self,
        request: StartWorkflowRunRequest,
        events: &mut S,
    ) -> AppResult<WorkflowRunDetails> {
        let definition = self.preflight(&request).await?;
        let start_node = definition
            .nodes
            .iter()
            .find(|node| matches!(node.config, WorkflowNodeConfig::Start {}))
            .map(|node| node.id)
            .ok_or_else(|| AppError::Validation("工作流缺少开始节点".into()))?;
        let run = self
            .workflows
            .create_run(NewWorkflowRun {
                workflow_id: definition.id,
                workflow_version: definition.version,
                server_id: request.server_id.clone(),
            })
            .await?;
        let cancel = self.registry.register(run.id).await?;
        self.workflows
            .mark_run_running(run.id, start_node, now_millis())
            .await?;
        self.emit(
            events,
            run.id,
            WorkflowEventPayload::RunStarted {
                run_id: run.id,
                workflow_id: definition.id,
                server_id: request.server_id.clone(),
            },
        )
        .await?;

        let mut current = start_node;
        let mut visited = HashSet::new();
        let mut contexts = HashMap::new();
        loop {
            self.workflows.set_current_node(run.id, current).await?;
            let node = definition
                .nodes
                .iter()
                .find(|node| node.id == current)
                .ok_or_else(|| AppError::Validation("工作流节点不存在".into()))?;
            let attempt = self
                .workflows
                .start_node_attempt(run.id, current, now_millis())
                .await?;
            self.emit(
                events,
                run.id,
                WorkflowEventPayload::NodeStarted {
                    run_id: run.id,
                    node_id: current,
                    attempt: attempt.attempt,
                },
            )
            .await?;

            let outcome = if cancel.is_cancelled() {
                cancelled_outcome()
            } else {
                self.execute_node(
                    &node.config,
                    NodeExecutionContext {
                        run_id: run.id,
                        node_id: node.id,
                        server_id: &request.server_id,
                        dangerous_confirmed: request.dangerous_confirmed,
                        contexts: &contexts,
                        cancel: cancel.clone(),
                        events,
                    },
                )
                .await
            };
            if let Some(execution_id) = outcome.execution_id {
                self.workflows
                    .link_node_execution(run.id, current, attempt.attempt, execution_id)
                    .await?;
            }
            self.workflows
                .finish_node(FinishWorkflowNode {
                    run_id: run.id,
                    node_id: current,
                    attempt: attempt.attempt,
                    status: outcome.status,
                    finished_at: now_millis(),
                    exit_code: outcome.exit_code,
                    result: outcome.result.clone(),
                    output_summary: outcome.output_summary.clone(),
                    error_message: outcome.error_message.clone(),
                    retryable: outcome.retryable,
                })
                .await?;
            self.emit(
                events,
                run.id,
                WorkflowEventPayload::NodeStatusChanged {
                    run_id: run.id,
                    node_id: current,
                    attempt: attempt.attempt,
                    status: outcome.status,
                    execution_id: outcome.execution_id,
                    message: outcome.error_message.clone(),
                },
            )
            .await?;
            visited.insert(current);

            if outcome.status != WorkflowNodeStatus::Succeeded {
                let run_status = match outcome.status {
                    WorkflowNodeStatus::Failed => WorkflowRunStatus::Paused,
                    WorkflowNodeStatus::Cancelled => WorkflowRunStatus::Cancelled,
                    WorkflowNodeStatus::Uncertain => WorkflowRunStatus::Uncertain,
                    _ => WorkflowRunStatus::Paused,
                };
                self.finish_run(
                    events,
                    run.id,
                    run_status,
                    outcome.error_category,
                    outcome.error_message,
                    outcome.retryable,
                )
                .await?;
                self.registry.remove(run.id).await;
                return self.require_run(run.id).await;
            }

            contexts.insert(
                current,
                ConditionContext {
                    exit_code: outcome.exit_code,
                    result: outcome.result,
                    output_summary: outcome.output_summary,
                },
            );
            if let Some(result) = outcome.condition_result {
                self.emit(
                    events,
                    run.id,
                    WorkflowEventPayload::ConditionEvaluated {
                        run_id: run.id,
                        node_id: current,
                        result,
                    },
                )
                .await?;
            }
            let next = next_node(&definition, current, outcome.condition_result)?;
            let Some(next) = next else {
                self.record_skipped(&definition, run.id, &visited, events)
                    .await?;
                self.finish_run(
                    events,
                    run.id,
                    WorkflowRunStatus::Succeeded,
                    None,
                    None,
                    false,
                )
                .await?;
                self.registry.remove(run.id).await;
                return self.require_run(run.id).await;
            };
            current = next;
        }
    }

    pub async fn cancel(&self, run_id: Uuid) -> AppResult<()> {
        let child = self.registry.cancel(run_id).await?;
        if let Some(execution_id) = child {
            self.execution_nodes.cancel(execution_id).await?;
        }
        Ok(())
    }

    pub async fn current_child(&self, run_id: Uuid) -> Option<Uuid> {
        self.registry.current_child(run_id).await
    }

    pub async fn is_idle(&self) -> bool {
        self.registry.is_empty().await
    }

    pub async fn retry_failed_node<S: WorkflowEventSink>(
        &self,
        run_id: Uuid,
        dangerous_confirmed: bool,
        events: &mut S,
    ) -> AppResult<WorkflowRunDetails> {
        let details = self.require_run(run_id).await?;
        if details.run.status != WorkflowRunStatus::Paused || !details.run.retryable {
            return Err(AppError::Validation(
                "只有可重试的暂停工作流才能重试".into(),
            ));
        }
        let current = details
            .run
            .current_node_id
            .ok_or_else(|| AppError::Validation("暂停工作流缺少失败节点".into()))?;
        let latest = latest_node_runs(&details);
        let failed = latest
            .get(&current)
            .ok_or_else(|| AppError::Validation("暂停工作流缺少失败节点记录".into()))?;
        if failed.status != WorkflowNodeStatus::Failed || !failed.retryable {
            return Err(AppError::Validation("当前失败节点不可重试".into()));
        }
        let request = StartWorkflowRunRequest {
            workflow_id: details.run.workflow_id,
            workflow_version: Some(details.run.workflow_version),
            server_id: details.run.server_id.clone(),
            dangerous_confirmed,
        };
        let definition = self.preflight(&request).await?;
        let mut contexts = HashMap::new();
        let mut visited = HashSet::new();
        for (node_id, node_run) in latest {
            if node_run.status == WorkflowNodeStatus::Succeeded {
                contexts.insert(
                    node_id,
                    ConditionContext {
                        exit_code: node_run.exit_code,
                        result: node_run.result.clone(),
                        output_summary: node_run.output_summary.clone(),
                    },
                );
                visited.insert(node_id);
            } else if node_run.status == WorkflowNodeStatus::Skipped {
                visited.insert(node_id);
            }
        }
        self.workflows
            .resume_paused_run(run_id, now_millis())
            .await?;
        let cancel = self.registry.register(run_id).await?;
        self.emit(
            events,
            run_id,
            WorkflowEventPayload::RunStatusChanged {
                run_id,
                status: WorkflowRunStatus::Running,
                message: Some("正在重试失败节点".into()),
            },
        )
        .await?;
        let result = self
            .continue_retry(RunContinuation {
                definition: &definition,
                run_id,
                server_id: &request.server_id,
                dangerous_confirmed,
                current,
                contexts,
                visited,
                cancel,
                events,
            })
            .await;
        self.registry.remove(run_id).await;
        result
    }

    async fn continue_retry<S: WorkflowEventSink>(
        &self,
        continuation: RunContinuation<'_, S>,
    ) -> AppResult<WorkflowRunDetails> {
        let RunContinuation {
            definition,
            run_id,
            server_id,
            dangerous_confirmed,
            mut current,
            mut contexts,
            mut visited,
            cancel,
            events,
        } = continuation;
        loop {
            self.workflows.set_current_node(run_id, current).await?;
            let node = definition
                .nodes
                .iter()
                .find(|node| node.id == current)
                .ok_or_else(|| AppError::Validation("工作流节点不存在".into()))?;
            let attempt = self
                .workflows
                .start_node_attempt(run_id, current, now_millis())
                .await?;
            self.emit(
                events,
                run_id,
                WorkflowEventPayload::NodeStarted {
                    run_id,
                    node_id: current,
                    attempt: attempt.attempt,
                },
            )
            .await?;
            let outcome = if cancel.is_cancelled() {
                cancelled_outcome()
            } else {
                self.execute_node(
                    &node.config,
                    NodeExecutionContext {
                        run_id,
                        node_id: node.id,
                        server_id,
                        dangerous_confirmed,
                        contexts: &contexts,
                        cancel: cancel.clone(),
                        events,
                    },
                )
                .await
            };
            if let Some(execution_id) = outcome.execution_id {
                self.workflows
                    .link_node_execution(run_id, current, attempt.attempt, execution_id)
                    .await?;
            }
            self.workflows
                .finish_node(FinishWorkflowNode {
                    run_id,
                    node_id: current,
                    attempt: attempt.attempt,
                    status: outcome.status,
                    finished_at: now_millis(),
                    exit_code: outcome.exit_code,
                    result: outcome.result.clone(),
                    output_summary: outcome.output_summary.clone(),
                    error_message: outcome.error_message.clone(),
                    retryable: outcome.retryable,
                })
                .await?;
            self.emit(
                events,
                run_id,
                WorkflowEventPayload::NodeStatusChanged {
                    run_id,
                    node_id: current,
                    attempt: attempt.attempt,
                    status: outcome.status,
                    execution_id: outcome.execution_id,
                    message: outcome.error_message.clone(),
                },
            )
            .await?;
            visited.insert(current);
            if outcome.status != WorkflowNodeStatus::Succeeded {
                let status = match outcome.status {
                    WorkflowNodeStatus::Failed => WorkflowRunStatus::Paused,
                    WorkflowNodeStatus::Cancelled => WorkflowRunStatus::Cancelled,
                    WorkflowNodeStatus::Uncertain => WorkflowRunStatus::Uncertain,
                    _ => WorkflowRunStatus::Paused,
                };
                self.finish_run(
                    events,
                    run_id,
                    status,
                    outcome.error_category,
                    outcome.error_message,
                    outcome.retryable,
                )
                .await?;
                return self.require_run(run_id).await;
            }
            contexts.insert(
                current,
                ConditionContext {
                    exit_code: outcome.exit_code,
                    result: outcome.result,
                    output_summary: outcome.output_summary,
                },
            );
            if let Some(result) = outcome.condition_result {
                self.emit(
                    events,
                    run_id,
                    WorkflowEventPayload::ConditionEvaluated {
                        run_id,
                        node_id: current,
                        result,
                    },
                )
                .await?;
            }
            let Some(next) = next_node(definition, current, outcome.condition_result)? else {
                self.record_skipped(definition, run_id, &visited, events)
                    .await?;
                self.finish_run(
                    events,
                    run_id,
                    WorkflowRunStatus::Succeeded,
                    None,
                    None,
                    false,
                )
                .await?;
                return self.require_run(run_id).await;
            };
            current = next;
        }
    }

    async fn preflight(&self, request: &StartWorkflowRunRequest) -> AppResult<WorkflowDefinition> {
        if request.server_id.trim().is_empty() {
            return Err(AppError::Validation("必须选择服务器".into()));
        }
        let definition = self
            .workflows
            .get(request.workflow_id, request.workflow_version)
            .await?
            .ok_or_else(|| AppError::Validation("工作流或指定版本不存在".into()))?;
        require_valid_workflow(&WorkflowDraft::from(definition.clone()))?;
        self.connector
            .require_trusted_server(&request.server_id)
            .await?;
        if requires_dangerous_confirmation(&definition) && !request.dangerous_confirmed {
            return Err(AppError::Validation(
                "工作流包含危险步骤，必须二次确认".into(),
            ));
        }
        if has_remote_nodes(&definition) {
            let connected = self.connector.connect(&request.server_id).await?;
            let compatibility: AppResult<()> = definition.nodes.iter().try_for_each(|node| {
                if let WorkflowNodeConfig::Task {
                    task_id,
                    task_version,
                    ..
                } = &node.config
                {
                    let task = built_in_catalog()
                        .into_iter()
                        .find(|task| {
                            task.id == *task_id && task_version_is_compatible(task, *task_version)
                        })
                        .ok_or_else(|| AppError::Validation("工作流任务版本不存在".into()))?;
                    select_implementation(&task, &connected.capabilities)?;
                }
                Ok(())
            });
            connected.session.disconnect().await;
            compatibility?;
        }
        Ok(definition)
    }

    async fn execute_node<S: WorkflowEventSink>(
        &self,
        config: &WorkflowNodeConfig,
        context: NodeExecutionContext<'_, S>,
    ) -> StepOutcome {
        match config {
            WorkflowNodeConfig::Start {} => {
                StepOutcome::succeeded("工作流已开始", Some(json!({"started": true})))
            }
            WorkflowNodeConfig::Stop { message } => {
                StepOutcome::succeeded(message.clone(), Some(json!({"stopped": true})))
            }
            WorkflowNodeConfig::Condition {
                source_node_id,
                predicate,
            } => match context
                .contexts
                .get(source_node_id)
                .ok_or_else(|| AppError::Validation("条件来源节点尚无结果".into()))
                .and_then(|source| evaluate_condition(predicate, source))
            {
                Ok(result) => {
                    let mut outcome = StepOutcome::succeeded(
                        if result {
                            "条件为真"
                        } else {
                            "条件为假"
                        },
                        Some(json!({"value": result})),
                    );
                    outcome.condition_result = Some(result);
                    outcome
                }
                Err(error) => StepOutcome::failed(error),
            },
            WorkflowNodeConfig::Task { .. } | WorkflowNodeConfig::Custom { .. } => {
                let mut child_events =
                    ChildTrackingSink::new(self.registry.clone(), context.run_id);
                let result = self
                    .execution_nodes
                    .execute(
                        context.server_id,
                        config,
                        context.dangerous_confirmed,
                        &mut child_events,
                    )
                    .await;
                if let Err(error) = child_events.finish() {
                    return StepOutcome::failed(error);
                }
                match result {
                    Ok(outcome) => outcome.into(),
                    Err(error) => StepOutcome::failed(error),
                }
            }
            WorkflowNodeConfig::Upload {
                local_path,
                remote_path,
                overwrite,
                create_restore_point,
            } => {
                if *create_restore_point {
                    match self
                        .restore_points
                        .capture(
                            context.run_id,
                            context.node_id,
                            context.server_id,
                            remote_path,
                            context.cancel,
                        )
                        .await
                    {
                        Ok(restore_point) => {
                            if let Err(error) = self
                                .emit(
                                    context.events,
                                    context.run_id,
                                    WorkflowEventPayload::RestorePointChanged {
                                        run_id: context.run_id,
                                        node_id: context.node_id,
                                        restore_point_id: restore_point.id,
                                        status: restore_point.status.as_str().into(),
                                    },
                                )
                                .await
                            {
                                return StepOutcome::failed(error);
                            }
                        }
                        Err(error) => return StepOutcome::failed(error),
                    }
                }
                let upload = WorkflowNodeConfig::Upload {
                    local_path: local_path.clone(),
                    remote_path: remote_path.clone(),
                    overwrite: *overwrite,
                    create_restore_point: false,
                };
                let mut child_events =
                    ChildTrackingSink::new(self.registry.clone(), context.run_id);
                let result = self
                    .io_nodes
                    .execute(context.server_id, &upload, &mut child_events)
                    .await;
                if let Err(error) = child_events.finish() {
                    return StepOutcome::failed(error);
                }
                match result {
                    Ok(outcome) => outcome.into(),
                    Err(error) => StepOutcome::failed(error),
                }
            }
            WorkflowNodeConfig::Download { .. } | WorkflowNodeConfig::LogSearch { .. } => {
                let mut child_events =
                    ChildTrackingSink::new(self.registry.clone(), context.run_id);
                let result = self
                    .io_nodes
                    .execute(context.server_id, config, &mut child_events)
                    .await;
                if let Err(error) = child_events.finish() {
                    return StepOutcome::failed(error);
                }
                match result {
                    Ok(outcome) => outcome.into(),
                    Err(error) => StepOutcome::failed(error),
                }
            }
        }
    }

    async fn record_skipped<S: WorkflowEventSink>(
        &self,
        definition: &WorkflowDefinition,
        run_id: Uuid,
        visited: &HashSet<Uuid>,
        events: &mut S,
    ) -> AppResult<()> {
        for node in &definition.nodes {
            if visited.contains(&node.id) {
                continue;
            }
            let skipped = self.workflows.record_skipped_node(run_id, node.id).await?;
            self.emit(
                events,
                run_id,
                WorkflowEventPayload::NodeStatusChanged {
                    run_id,
                    node_id: node.id,
                    attempt: skipped.attempt,
                    status: WorkflowNodeStatus::Skipped,
                    execution_id: None,
                    message: Some("未选择该分支".into()),
                },
            )
            .await?;
        }
        Ok(())
    }

    async fn finish_run<S: WorkflowEventSink>(
        &self,
        events: &mut S,
        run_id: Uuid,
        status: WorkflowRunStatus,
        error_category: Option<String>,
        error_message: Option<String>,
        retryable: bool,
    ) -> AppResult<()> {
        self.workflows
            .finish_run(FinishWorkflowRun {
                run_id,
                status,
                finished_at: now_millis(),
                error_category,
                error_message: error_message.clone(),
                retryable,
            })
            .await?;
        self.emit(
            events,
            run_id,
            WorkflowEventPayload::RunStatusChanged {
                run_id,
                status,
                message: error_message,
            },
        )
        .await?;
        let duration_ms = self
            .workflows
            .get_run(run_id)
            .await?
            .and_then(|details| details.run.duration_ms)
            .unwrap_or_default();
        self.emit(
            events,
            run_id,
            WorkflowEventPayload::Finished {
                run_id,
                status,
                duration_ms,
            },
        )
        .await
    }

    async fn emit<S: WorkflowEventSink>(
        &self,
        events: &mut S,
        run_id: Uuid,
        payload: WorkflowEventPayload,
    ) -> AppResult<()> {
        let raw = serde_json::to_value(payload)
            .map_err(|_| AppError::Serialization("工作流事件无法序列化".into()))?;
        let redacted = Redactor::default().redact_json(&raw);
        let event_type = redacted
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::Serialization("工作流事件缺少类型".into()))?;
        let emitted_at = now_millis();
        let persisted = self
            .workflows
            .append_event(run_id, event_type, redacted.clone(), emitted_at)
            .await?;
        let payload = serde_json::from_value(redacted)
            .map_err(|_| AppError::Serialization("脱敏后的工作流事件无效".into()))?;
        events.send(WorkflowEvent {
            sequence: u64::try_from(persisted.sequence)
                .map_err(|_| AppError::Validation("工作流事件序号无效".into()))?,
            emitted_at,
            payload,
        })
    }

    async fn require_run(&self, run_id: Uuid) -> AppResult<WorkflowRunDetails> {
        self.workflows
            .get_run(run_id)
            .await?
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }
}

fn cancelled_outcome() -> StepOutcome {
    StepOutcome {
        status: WorkflowNodeStatus::Cancelled,
        execution_id: None,
        exit_code: None,
        result: None,
        output_summary: None,
        error_category: Some("cancelled".into()),
        error_message: Some(AppError::Cancelled.to_string()),
        retryable: false,
        condition_result: None,
    }
}

fn latest_node_runs(details: &WorkflowRunDetails) -> HashMap<Uuid, &WorkflowNodeRun> {
    let mut latest = HashMap::new();
    for node_run in &details.node_runs {
        let replace = latest
            .get(&node_run.node_id)
            .is_none_or(|current: &&WorkflowNodeRun| current.attempt < node_run.attempt);
        if replace {
            latest.insert(node_run.node_id, node_run);
        }
    }
    latest
}

fn next_node(
    definition: &WorkflowDefinition,
    current: Uuid,
    condition_result: Option<bool>,
) -> AppResult<Option<Uuid>> {
    let branch = condition_result.map_or(WorkflowEdgeBranch::Success, |result| {
        if result {
            WorkflowEdgeBranch::True
        } else {
            WorkflowEdgeBranch::False
        }
    });
    let matches = definition
        .edges
        .iter()
        .filter(|edge| edge.from == current && edge.branch == branch)
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        return Err(AppError::Validation("工作流下一节点不唯一".into()));
    }
    Ok(matches.first().map(|edge| edge.to))
}

fn has_remote_nodes(definition: &WorkflowDefinition) -> bool {
    definition.nodes.iter().any(|node| {
        matches!(
            node.config,
            WorkflowNodeConfig::Task { .. }
                | WorkflowNodeConfig::Custom { .. }
                | WorkflowNodeConfig::Upload { .. }
                | WorkflowNodeConfig::Download { .. }
                | WorkflowNodeConfig::LogSearch { .. }
        )
    })
}

fn requires_dangerous_confirmation(definition: &WorkflowDefinition) -> bool {
    definition.nodes.iter().any(|node| match &node.config {
        WorkflowNodeConfig::Custom { .. } => true,
        WorkflowNodeConfig::Upload { overwrite, .. } => *overwrite,
        WorkflowNodeConfig::Task { task_id, .. } => built_in_catalog()
            .into_iter()
            .find(|task| task.id == *task_id)
            .is_some_and(|task| task.risk_level == RiskLevel::Dangerous),
        _ => false,
    })
}
