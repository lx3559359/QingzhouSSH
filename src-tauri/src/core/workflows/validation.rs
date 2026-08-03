use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::Path,
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{
    core::{
        logs::LogSearchRequest,
        sftp::{download_destination, validate_remote_path},
        tasks::{built_in_catalog, validate_parameters},
        workflows::condition::validate_condition,
    },
    domain::workflow::{
        WorkflowDraft, WorkflowEdge, WorkflowEdgeBranch, WorkflowNode, WorkflowNodeConfig,
    },
    error::{AppError, AppResult},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowDiagnosticCode {
    GraphLimit,
    DuplicateNode,
    StartCount,
    StartEdges,
    StopEdges,
    MissingNode,
    SelfEdge,
    DuplicateEdge,
    InvalidBranch,
    ConditionBranches,
    Cycle,
    UnreachableNode,
    NoTerminalPath,
    InvalidParameters,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDiagnostic {
    pub code: WorkflowDiagnosticCode,
    pub node_id: Option<Uuid>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowValidationReport {
    pub valid: bool,
    pub start_node_id: Option<Uuid>,
    pub diagnostics: Vec<WorkflowDiagnostic>,
}

pub fn require_valid_workflow(draft: &WorkflowDraft) -> AppResult<()> {
    let report = validate_workflow(draft);
    if report.valid {
        Ok(())
    } else {
        Err(AppError::Validation(
            report
                .diagnostics
                .first()
                .map(|diagnostic| diagnostic.message.clone())
                .unwrap_or_else(|| "工作流校验失败".into()),
        ))
    }
}

pub fn validate_workflow(draft: &WorkflowDraft) -> WorkflowValidationReport {
    let mut diagnostics = Vec::new();
    if draft.nodes.len() > 100 || draft.edges.len() > 200 {
        push(
            &mut diagnostics,
            WorkflowDiagnosticCode::GraphLimit,
            None,
            "工作流最多允许 100 个节点和 200 条边",
        );
    }

    let mut nodes = HashMap::new();
    for node in &draft.nodes {
        if nodes.insert(node.id, node).is_some() {
            push(
                &mut diagnostics,
                WorkflowDiagnosticCode::DuplicateNode,
                Some(node.id),
                "节点标识重复",
            );
        }
        if let Err(error) = validate_node(node) {
            push(
                &mut diagnostics,
                WorkflowDiagnosticCode::InvalidParameters,
                Some(node.id),
                &error.to_string(),
            );
        }
    }

    let starts = draft
        .nodes
        .iter()
        .filter(|node| matches!(node.config, WorkflowNodeConfig::Start {}))
        .collect::<Vec<_>>();
    if starts.len() != 1 {
        push(
            &mut diagnostics,
            WorkflowDiagnosticCode::StartCount,
            None,
            "工作流必须且只能包含一个开始节点",
        );
    }
    let start_node_id = (starts.len() == 1).then(|| starts[0].id);

    let mut outgoing: HashMap<Uuid, Vec<_>> = HashMap::new();
    let mut incoming: HashMap<Uuid, Vec<_>> = HashMap::new();
    let mut unique_edges = HashSet::new();
    for edge in &draft.edges {
        let from_exists = nodes.contains_key(&edge.from);
        let to_exists = nodes.contains_key(&edge.to);
        if !from_exists || !to_exists {
            push(
                &mut diagnostics,
                WorkflowDiagnosticCode::MissingNode,
                Some(edge.from),
                "连接引用了不存在的节点",
            );
            continue;
        }
        if edge.from == edge.to {
            push(
                &mut diagnostics,
                WorkflowDiagnosticCode::SelfEdge,
                Some(edge.from),
                "节点不能连接到自身",
            );
        }
        if !unique_edges.insert((edge.from, edge.to, edge.branch)) {
            push(
                &mut diagnostics,
                WorkflowDiagnosticCode::DuplicateEdge,
                Some(edge.from),
                "存在重复连接",
            );
        }
        outgoing.entry(edge.from).or_default().push(edge);
        incoming.entry(edge.to).or_default().push(edge);
    }

    for node in &draft.nodes {
        let out = outgoing
            .get(&node.id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let in_count = incoming.get(&node.id).map_or(0, Vec::len);
        match &node.config {
            WorkflowNodeConfig::Start {} => {
                if in_count != 0 || out.len() != 1 || out[0].branch != WorkflowEdgeBranch::Success {
                    push(
                        &mut diagnostics,
                        WorkflowDiagnosticCode::StartEdges,
                        Some(node.id),
                        "开始节点必须无入边且只有一条成功出边",
                    );
                }
            }
            WorkflowNodeConfig::Stop { .. } => {
                if !out.is_empty() {
                    push(
                        &mut diagnostics,
                        WorkflowDiagnosticCode::StopEdges,
                        Some(node.id),
                        "停止节点不能有出边",
                    );
                }
            }
            WorkflowNodeConfig::Condition { source_node_id, .. } => {
                let has_true = out
                    .iter()
                    .filter(|edge| edge.branch == WorkflowEdgeBranch::True)
                    .count()
                    == 1;
                let has_false = out
                    .iter()
                    .filter(|edge| edge.branch == WorkflowEdgeBranch::False)
                    .count()
                    == 1;
                let distinct_targets = out.len() == 2 && out[0].to != out[1].to;
                if out.len() != 2 || !has_true || !has_false || !distinct_targets {
                    push(
                        &mut diagnostics,
                        WorkflowDiagnosticCode::ConditionBranches,
                        Some(node.id),
                        "条件节点必须有目标不同的真、假分支",
                    );
                }
                if !nodes.contains_key(source_node_id) || *source_node_id == node.id {
                    push(
                        &mut diagnostics,
                        WorkflowDiagnosticCode::InvalidParameters,
                        Some(node.id),
                        "条件来源节点无效",
                    );
                }
            }
            _ => {
                if out.len() > 1
                    || out
                        .first()
                        .is_some_and(|edge| edge.branch != WorkflowEdgeBranch::Success)
                {
                    push(
                        &mut diagnostics,
                        WorkflowDiagnosticCode::InvalidBranch,
                        Some(node.id),
                        "普通节点最多只能有一条成功出边",
                    );
                }
            }
        }
    }

    if has_cycle(&draft.nodes, &outgoing) {
        push(
            &mut diagnostics,
            WorkflowDiagnosticCode::Cycle,
            None,
            "工作流不允许循环",
        );
    }

    if let Some(start) = start_node_id {
        let reachable = reachable_from(start, &outgoing);
        for node in &draft.nodes {
            if !reachable.contains(&node.id) {
                push(
                    &mut diagnostics,
                    WorkflowDiagnosticCode::UnreachableNode,
                    Some(node.id),
                    "节点无法从开始节点到达",
                );
            }
        }
        let has_terminal = draft.nodes.iter().any(|node| {
            reachable.contains(&node.id)
                && outgoing.get(&node.id).is_none_or(Vec::is_empty)
                && !matches!(
                    node.config,
                    WorkflowNodeConfig::Start {} | WorkflowNodeConfig::Condition { .. }
                )
        });
        if !has_terminal {
            push(
                &mut diagnostics,
                WorkflowDiagnosticCode::NoTerminalPath,
                None,
                "工作流没有可到达的终止路径",
            );
        }
    }

    WorkflowValidationReport {
        valid: diagnostics.is_empty(),
        start_node_id,
        diagnostics,
    }
}

fn validate_node(node: &WorkflowNode) -> AppResult<()> {
    if node.name.trim().is_empty()
        || node.name.chars().count() > 200
        || !node.position.x.is_finite()
        || !node.position.y.is_finite()
    {
        return Err(AppError::Validation("节点名称或位置无效".into()));
    }
    match &node.config {
        WorkflowNodeConfig::Start {} => Ok(()),
        WorkflowNodeConfig::Task {
            task_id,
            task_version,
            parameters,
        } => {
            let definition = built_in_catalog()
                .into_iter()
                .find(|definition| definition.id == *task_id && definition.version == *task_version)
                .ok_or_else(|| AppError::Validation("工作流任务 ID 或版本不存在".into()))?;
            let object = Map::from_iter(parameters.clone());
            validate_parameters(&definition, &Value::Object(object)).map(|_| ())
        }
        WorkflowNodeConfig::Custom {
            content,
            timeout_seconds,
            ..
        } => {
            if content.trim().is_empty()
                || content.contains('\0')
                || content.len() > 1024 * 1024
                || !(1..=3_600).contains(timeout_seconds)
            {
                Err(AppError::Validation("高级命令或脚本参数无效".into()))
            } else {
                Ok(())
            }
        }
        WorkflowNodeConfig::Upload {
            local_path,
            remote_path,
            overwrite,
            create_restore_point,
        } => {
            if !Path::new(local_path).is_absolute() || local_path.contains('\0') {
                return Err(AppError::Validation("上传源必须是绝对本地路径".into()));
            }
            if *create_restore_point && !*overwrite {
                return Err(AppError::Validation("只有覆盖上传才能创建恢复点".into()));
            }
            validate_remote_path(remote_path)
        }
        WorkflowNodeConfig::Download {
            remote_path,
            suggested_name,
            ..
        } => {
            validate_remote_path(remote_path)?;
            download_destination(Path::new("D:/workflow-data"), suggested_name).map(|_| ())
        }
        WorkflowNodeConfig::LogSearch {
            path,
            keyword,
            case_sensitive,
            context_lines,
            limit,
            start_time,
            end_time,
        } => LogSearchRequest {
            path: path.clone(),
            keyword: keyword.clone(),
            case_sensitive: *case_sensitive,
            context_lines: *context_lines,
            limit: *limit,
            start_time: start_time.clone(),
            end_time: end_time.clone(),
        }
        .validate(),
        WorkflowNodeConfig::Condition { predicate, .. } => validate_condition(predicate),
        WorkflowNodeConfig::Stop { message } => {
            if message.is_empty() || message.len() > 4096 || message.contains('\0') {
                Err(AppError::Validation("停止提示必须为 1 到 4096 字节".into()))
            } else {
                Ok(())
            }
        }
    }
}

fn has_cycle(nodes: &[WorkflowNode], outgoing: &HashMap<Uuid, Vec<&WorkflowEdge>>) -> bool {
    let mut indegree = nodes
        .iter()
        .map(|node| (node.id, 0_usize))
        .collect::<HashMap<_, _>>();
    for edges in outgoing.values() {
        for edge in edges {
            if let Some(value) = indegree.get_mut(&edge.to) {
                *value += 1;
            }
        }
    }
    let mut queue = indegree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
        .collect::<VecDeque<_>>();
    let mut visited = 0;
    while let Some(id) = queue.pop_front() {
        visited += 1;
        for edge in outgoing.get(&id).map(Vec::as_slice).unwrap_or_default() {
            if let Some(value) = indegree.get_mut(&edge.to) {
                *value -= 1;
                if *value == 0 {
                    queue.push_back(edge.to);
                }
            }
        }
    }
    visited != nodes.len()
}

fn reachable_from(start: Uuid, outgoing: &HashMap<Uuid, Vec<&WorkflowEdge>>) -> HashSet<Uuid> {
    let mut reachable = HashSet::new();
    let mut queue = VecDeque::from([start]);
    while let Some(id) = queue.pop_front() {
        if !reachable.insert(id) {
            continue;
        }
        for edge in outgoing.get(&id).map(Vec::as_slice).unwrap_or_default() {
            queue.push_back(edge.to);
        }
    }
    reachable
}

fn push(
    diagnostics: &mut Vec<WorkflowDiagnostic>,
    code: WorkflowDiagnosticCode,
    node_id: Option<Uuid>,
    message: &str,
) {
    diagnostics.push(WorkflowDiagnostic {
        code,
        node_id,
        message: message.into(),
    });
}
