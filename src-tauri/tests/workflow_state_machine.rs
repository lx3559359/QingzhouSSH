use qingzhou_ssh_lib::{
    core::{
        redaction::Redactor,
        workflows::{validate_node_transition, validate_run_transition},
    },
    domain::{
        workflow::{WorkflowNodeStatus, WorkflowRunStatus},
        workflow_events::{VecWorkflowEventSink, WorkflowEventEmitter, WorkflowEventPayload},
    },
    services::workflow_registry::WorkflowRunRegistry,
};
use uuid::Uuid;

#[test]
fn permits_only_explicit_run_and_node_state_transitions() {
    for (from, to) in [
        (WorkflowRunStatus::Queued, WorkflowRunStatus::Running),
        (WorkflowRunStatus::Running, WorkflowRunStatus::Paused),
        (WorkflowRunStatus::Paused, WorkflowRunStatus::Running),
        (WorkflowRunStatus::Running, WorkflowRunStatus::Succeeded),
        (WorkflowRunStatus::Running, WorkflowRunStatus::Cancelled),
        (WorkflowRunStatus::Running, WorkflowRunStatus::Uncertain),
        (WorkflowRunStatus::Paused, WorkflowRunStatus::RolledBack),
        (WorkflowRunStatus::Paused, WorkflowRunStatus::RollbackFailed),
    ] {
        validate_run_transition(from, to).unwrap();
    }
    assert!(
        validate_run_transition(WorkflowRunStatus::Queued, WorkflowRunStatus::Succeeded).is_err()
    );
    assert!(
        validate_run_transition(WorkflowRunStatus::Succeeded, WorkflowRunStatus::Running).is_err()
    );

    for terminal in [
        WorkflowNodeStatus::Succeeded,
        WorkflowNodeStatus::Failed,
        WorkflowNodeStatus::Cancelled,
        WorkflowNodeStatus::Uncertain,
    ] {
        validate_node_transition(WorkflowNodeStatus::Running, terminal).unwrap();
    }
    validate_node_transition(WorkflowNodeStatus::Pending, WorkflowNodeStatus::Running).unwrap();
    validate_node_transition(WorkflowNodeStatus::Pending, WorkflowNodeStatus::Skipped).unwrap();
    assert!(
        validate_node_transition(WorkflowNodeStatus::Succeeded, WorkflowNodeStatus::Running)
            .is_err()
    );
}

#[test]
fn emits_monotonic_redacted_bounded_workflow_events() {
    let run_id = Uuid::new_v4();
    let node_id = Uuid::new_v4();
    let mut sink = VecWorkflowEventSink::default();
    let redactor = Redactor::new(["workflow-secret-canary"]);
    let mut emitter = WorkflowEventEmitter::new(&mut sink, redactor);

    emitter
        .emit(WorkflowEventPayload::RunStarted {
            run_id,
            workflow_id: Uuid::new_v4(),
            server_id: "server-1".into(),
        })
        .unwrap();
    emitter
        .emit(WorkflowEventPayload::NodeStatusChanged {
            run_id,
            node_id,
            attempt: 1,
            status: WorkflowNodeStatus::Failed,
            execution_id: None,
            message: Some("password=workflow-secret-canary".into()),
        })
        .unwrap();
    assert_eq!(sink.events.len(), 2);
    assert_eq!((sink.events[0].sequence, sink.events[1].sequence), (1, 2));
    let encoded = serde_json::to_string(&sink.events).unwrap();
    assert!(!encoded.contains("workflow-secret-canary"));
    assert!(encoded.contains("[REDACTED]"));

    let mut bounded_sink = VecWorkflowEventSink::default();
    let mut bounded = WorkflowEventEmitter::new(&mut bounded_sink, Redactor::default());
    let oversized = bounded.emit(WorkflowEventPayload::NodeStatusChanged {
        run_id,
        node_id,
        attempt: 1,
        status: WorkflowNodeStatus::Failed,
        execution_id: None,
        message: Some("x".repeat(33 * 1024)),
    });
    assert!(oversized.is_err());
    assert!(bounded_sink.events.is_empty());
}

#[tokio::test]
async fn registry_cancels_current_run_and_tracks_only_its_child_execution() {
    let registry = WorkflowRunRegistry::default();
    let run_id = Uuid::new_v4();
    let child = Uuid::new_v4();
    let token = registry.register(run_id).await.unwrap();
    assert!(registry.contains(run_id).await);
    registry.set_child(run_id, child).await.unwrap();
    assert_eq!(registry.current_child(run_id).await, Some(child));

    assert_eq!(registry.cancel(run_id).await.unwrap(), Some(child));
    assert!(token.is_cancelled());
    registry.clear_child(run_id, child).await.unwrap();
    assert_eq!(registry.current_child(run_id).await, None);
    registry.remove(run_id).await;
    assert!(!registry.contains(run_id).await);
    assert!(registry.cancel(run_id).await.is_err());
}
