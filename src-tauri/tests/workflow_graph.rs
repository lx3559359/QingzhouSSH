use std::collections::BTreeMap;

use qingzhou_ssh_lib::{
    core::workflows::{
        evaluate_condition, validate_workflow, ConditionContext, WorkflowDiagnosticCode,
    },
    domain::workflow::{
        EqualityOperator, NodePosition, NumericOperator, WorkflowCondition, WorkflowCustomMode,
        WorkflowDraft, WorkflowEdge, WorkflowEdgeBranch, WorkflowNode, WorkflowNodeConfig,
    },
};
use serde_json::json;
use uuid::Uuid;

fn node(id: Uuid, name: &str, config: WorkflowNodeConfig) -> WorkflowNode {
    WorkflowNode {
        id,
        name: name.into(),
        position: NodePosition { x: 10.0, y: 10.0 },
        config,
    }
}

fn valid_draft() -> WorkflowDraft {
    let start = Uuid::new_v4();
    let task = Uuid::new_v4();
    let condition = Uuid::new_v4();
    let yes = Uuid::new_v4();
    let no = Uuid::new_v4();
    WorkflowDraft {
        id: None,
        name: "有界条件工作流".into(),
        description: String::new(),
        nodes: vec![
            node(start, "开始", WorkflowNodeConfig::Start {}),
            node(
                task,
                "磁盘使用",
                WorkflowNodeConfig::Task {
                    task_id: "system.disk_usage".into(),
                    task_version: 1,
                    parameters: BTreeMap::new(),
                },
            ),
            node(
                condition,
                "检查退出码",
                WorkflowNodeConfig::Condition {
                    source_node_id: task,
                    predicate: WorkflowCondition::ExitCode {
                        operator: NumericOperator::Equal,
                        value: 0,
                    },
                },
            ),
            node(
                yes,
                "成功",
                WorkflowNodeConfig::Stop {
                    message: "检查通过".into(),
                },
            ),
            node(
                no,
                "停止",
                WorkflowNodeConfig::Stop {
                    message: "检查失败".into(),
                },
            ),
        ],
        edges: vec![
            WorkflowEdge {
                from: start,
                to: task,
                branch: WorkflowEdgeBranch::Success,
            },
            WorkflowEdge {
                from: task,
                to: condition,
                branch: WorkflowEdgeBranch::Success,
            },
            WorkflowEdge {
                from: condition,
                to: yes,
                branch: WorkflowEdgeBranch::True,
            },
            WorkflowEdge {
                from: condition,
                to: no,
                branch: WorkflowEdgeBranch::False,
            },
        ],
    }
}

fn codes(draft: &WorkflowDraft) -> Vec<WorkflowDiagnosticCode> {
    validate_workflow(draft)
        .diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn validates_a_bounded_reachable_acyclic_graph() {
    let draft = valid_draft();
    let report = validate_workflow(&draft);
    assert!(
        report.valid,
        "unexpected diagnostics: {:?}",
        report.diagnostics
    );
    assert_eq!(report.start_node_id, Some(draft.nodes[0].id));
}

#[test]
fn rejects_start_edge_branch_cycle_duplicate_missing_and_unreachable_nodes() {
    let mut missing_start = valid_draft();
    missing_start.nodes.remove(0);
    assert!(codes(&missing_start).contains(&WorkflowDiagnosticCode::StartCount));

    let mut duplicate = valid_draft();
    duplicate.edges.push(duplicate.edges[0].clone());
    assert!(codes(&duplicate).contains(&WorkflowDiagnosticCode::DuplicateEdge));

    let mut missing_reference = valid_draft();
    missing_reference.edges[0].to = Uuid::new_v4();
    assert!(codes(&missing_reference).contains(&WorkflowDiagnosticCode::MissingNode));

    let mut cycle = valid_draft();
    let stop = cycle.nodes[3].id;
    let task = cycle.nodes[1].id;
    cycle.edges.push(WorkflowEdge {
        from: stop,
        to: task,
        branch: WorkflowEdgeBranch::Success,
    });
    assert!(codes(&cycle).contains(&WorkflowDiagnosticCode::Cycle));

    let mut wrong_branch = valid_draft();
    wrong_branch.edges[2].branch = WorkflowEdgeBranch::Success;
    assert!(codes(&wrong_branch).contains(&WorkflowDiagnosticCode::ConditionBranches));

    let mut unreachable = valid_draft();
    unreachable.nodes.push(node(
        Uuid::new_v4(),
        "孤立节点",
        WorkflowNodeConfig::Stop {
            message: "孤立".into(),
        },
    ));
    assert!(codes(&unreachable).contains(&WorkflowDiagnosticCode::UnreachableNode));
}

#[test]
fn rejects_self_edges_limits_and_workflows_without_a_terminal_path() {
    let mut self_edge = valid_draft();
    let task = self_edge.nodes[1].id;
    self_edge.edges.push(WorkflowEdge {
        from: task,
        to: task,
        branch: WorkflowEdgeBranch::Success,
    });
    assert!(codes(&self_edge).contains(&WorkflowDiagnosticCode::SelfEdge));

    let mut too_many = valid_draft();
    for index in 0..96 {
        too_many.nodes.push(node(
            Uuid::new_v4(),
            &format!("额外节点 {index}"),
            WorkflowNodeConfig::Stop {
                message: "limit".into(),
            },
        ));
    }
    assert!(codes(&too_many).contains(&WorkflowDiagnosticCode::GraphLimit));

    let mut no_terminal = valid_draft();
    no_terminal
        .nodes
        .retain(|item| !matches!(item.config, WorkflowNodeConfig::Stop { .. }));
    no_terminal
        .edges
        .retain(|edge| no_terminal.nodes.iter().any(|node| node.id == edge.to));
    assert!(codes(&no_terminal).contains(&WorkflowDiagnosticCode::NoTerminalPath));
}

#[test]
fn reuses_m2_parameter_log_transfer_and_custom_validation() {
    let mut invalid_task = valid_draft();
    let task = invalid_task
        .nodes
        .iter_mut()
        .find(|node| matches!(node.config, WorkflowNodeConfig::Task { .. }))
        .unwrap();
    task.config = WorkflowNodeConfig::Task {
        task_id: "system.process_query".into(),
        task_version: 1,
        parameters: BTreeMap::from([("unknown".into(), json!("oops"))]),
    };
    assert!(codes(&invalid_task).contains(&WorkflowDiagnosticCode::InvalidParameters));

    let mut invalid_nodes = valid_draft();
    invalid_nodes.nodes[1].config = WorkflowNodeConfig::LogSearch {
        path: "relative.log".into(),
        keyword: "error".into(),
        case_sensitive: false,
        context_lines: 2,
        limit: 100,
        start_time: None,
        end_time: None,
    };
    assert!(codes(&invalid_nodes).contains(&WorkflowDiagnosticCode::InvalidParameters));

    invalid_nodes.nodes[1].config = WorkflowNodeConfig::Upload {
        local_path: "relative.zip".into(),
        remote_path: "/tmp/release.zip".into(),
        overwrite: true,
        create_restore_point: true,
    };
    assert!(codes(&invalid_nodes).contains(&WorkflowDiagnosticCode::InvalidParameters));

    invalid_nodes.nodes[1].config = WorkflowNodeConfig::Custom {
        mode: WorkflowCustomMode::Script,
        content: String::new(),
        timeout_seconds: 0,
    };
    assert!(codes(&invalid_nodes).contains(&WorkflowDiagnosticCode::InvalidParameters));
}

#[test]
fn evaluates_only_exit_code_json_field_and_fixed_output_conditions() {
    let context = ConditionContext {
        exit_code: Some(0),
        result: Some(json!({"service": {"healthy": true, "workers": 4}})),
        output_summary: Some("health check OK".into()),
    };
    assert!(evaluate_condition(
        &WorkflowCondition::ExitCode {
            operator: NumericOperator::Equal,
            value: 0,
        },
        &context,
    )
    .unwrap());
    assert!(evaluate_condition(
        &WorkflowCondition::ResultField {
            path: "service.healthy".into(),
            operator: EqualityOperator::Equal,
            value: json!(true),
        },
        &context,
    )
    .unwrap());
    assert!(evaluate_condition(
        &WorkflowCondition::OutputContains {
            text: "OK".into(),
            negated: false,
        },
        &context,
    )
    .unwrap());

    for invalid in ["service[0]", "service.$secret", "..", "a.*"] {
        let result = evaluate_condition(
            &WorkflowCondition::ResultField {
                path: invalid.into(),
                operator: EqualityOperator::Equal,
                value: json!(true),
            },
            &context,
        );
        assert!(result.is_err(), "invalid path accepted: {invalid}");
    }
    assert!(evaluate_condition(
        &WorkflowCondition::OutputContains {
            text: "x".repeat(513),
            negated: false,
        },
        &context,
    )
    .is_err());
}

#[test]
fn workflow_dtos_reject_unknown_fields_that_could_hide_secrets() {
    let value = json!({
        "id": Uuid::new_v4(),
        "name": "开始",
        "position": {"x": 0, "y": 0},
        "config": {"type": "start", "password": "must-not-hide-here"}
    });
    assert!(serde_json::from_value::<WorkflowNode>(value).is_err());
}
