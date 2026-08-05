import type {
  CreateServerRequest,
  CustomExecutionRequest,
  DownloadRequest,
  DirectoryListing,
  ExecutionDetails,
  ExecutionEvent,
  ExecutionFile,
  ExecutionFilter,
  LogSearchRequest,
  OperationFilter,
  OperationPreflightRequest,
  OperationPreview,
  OperationRunDetails,
  OperationRunRecord,
  OperationStartRequest,
  HostKeyCheck,
  HostKeyObservation,
  ServerProfile,
  SystemCapabilities,
  TaskAvailability,
  TaskExecutionRequest,
  UploadRequest,
  StartWorkflowRunRequest,
  WorkflowDefinition,
  WorkflowDiagnostic,
  WorkflowDraft,
  WorkflowEvent,
  WorkflowNode,
  WorkflowNodeRun,
  WorkflowRunDetails,
  WorkflowRunFilter,
  WorkflowRunRecord,
  WorkflowRunStatus,
  WorkflowSummary,
  WorkflowValidationReport,
  UpdateProgressEvent,
  UpdateSource,
  UpdateStatus,
} from './contracts';

const previewServer: ServerProfile = {
  id: 'preview-openeuler',
  name: 'openEuler 应用服务器',
  host: '192.168.10.28',
  port: 22,
  username: 'ops',
  authKind: 'private_key',
  credentialId: 'preview-credential',
};

let previewServers = [previewServer];
let previewExecutions: ExecutionDetails[] = [];
let previewLogTarget: LogSearchRequest['target'] = 'content';
const previewDataRoot =
  import.meta.env.VITE_QINGZHOU_DATA_ROOT ?? '.local\\dev-data（项目目录内）';

const previewTasks: TaskAvailability[] = [
  {
    compatible: true,
    reason: null,
    definition: {
      id: 'system.overview',
      version: 2,
      category: 'system',
      title: '系统概览',
      description: '查看运行时间、负载、内存和磁盘摘要',
      riskLevel: 'safe',
      estimatedSeconds: 30,
      privilege: 'current_user',
      scope: 'read_only_batch',
      parameters: [],
      implementations: [
        {
          id: 'posix',
          compatibility: {
            osFamilies: ['debian', 'rhel', 'openeuler'],
            serviceManagers: [],
            requiredCommands: ['uptime'],
          },
          preflightSteps: [],
          previewSteps: [{ id: 'preview', title: '执行预演', timeoutSeconds: 30, outputLimitBytes: 1048576 }],
          backupPlan: null,
          executionSteps: [{ id: 'execute', title: '执行任务', timeoutSeconds: 30, outputLimitBytes: 1048576 }],
          verifySteps: [],
          rollbackPlan: null,
          resultParser: 'key_value',
        },
      ],
      outputKind: 'key_value',
    },
  },
  {
    compatible: true,
    reason: null,
    definition: {
      id: 'service.restart',
      version: 2,
      category: 'service',
      title: '重启服务',
      description: '重启指定 systemd 服务',
      riskLevel: 'dangerous',
      estimatedSeconds: 30,
      privilege: 'current_user',
      scope: 'single_server',
      parameters: [
        {
          name: 'service',
          label: '服务名',
          description: '例如 nginx.service',
          kind: { type: 'serviceName' },
          required: true,
          defaultValue: null,
          sensitive: false,
        },
      ],
      implementations: [
        {
          id: 'systemd-restart',
          compatibility: {
            osFamilies: ['debian', 'rhel', 'openeuler'],
            serviceManagers: ['systemd'],
            requiredCommands: ['systemctl'],
          },
          preflightSteps: [],
          previewSteps: [{ id: 'preview', title: '查看当前服务状态', timeoutSeconds: 30, outputLimitBytes: 1048576 }],
          backupPlan: null,
          executionSteps: [{ id: 'execute', title: '执行任务', timeoutSeconds: 30, outputLimitBytes: 1048576 }],
          verifySteps: [],
          rollbackPlan: null,
          resultParser: 'service_status',
        },
      ],
      outputKind: 'text',
    },
  },
];

function createPreviewExecution(serverId: string, taskId: string): ExecutionDetails {
  const now = Date.now();
  const details: ExecutionDetails = {
    record: {
      id: `preview-execution-${previewExecutions.length + 1}`,
      serverId,
      taskId,
      taskVersion: 1,
      category: taskId.split('.')[0],
      status: 'succeeded',
      createdAt: now,
      startedAt: now,
      finishedAt: now + 360,
      durationMs: 360,
      exitCode: 0,
      errorCategory: null,
      errorMessage: null,
      retryable: false,
      parametersSummary: null,
      outputSummary: '预览执行成功',
      remoteProcessGroup: null,
    },
    parameters: [],
    files: [],
  };
  previewExecutions = [details, ...previewExecutions];
  return details;
}

function emitPreview(onEvent: (event: ExecutionEvent) => void, details: ExecutionDetails) {
  const startedAt = details.record.startedAt ?? Date.now();
  onEvent({
    type: 'started',
    sequence: 1,
    emittedAt: startedAt,
    executionId: details.record.id,
    startedAt,
  });
  onEvent({
    type: 'stdout',
    sequence: 2,
    emittedAt: startedAt + 100,
    text: 'QingzhouSSH 预览输出\n',
    totalBytes: 26,
  });
  onEvent({
    type: 'finished',
    sequence: 3,
    emittedAt: startedAt + 360,
    status: 'succeeded',
    exitCode: 0,
    durationMs: 360,
    result: null,
  });
}

let previewOperationCounter = 0;
let previewOperations = new Map<string, OperationRunDetails>();

function operationTask(taskId: string, taskVersion: number) {
  const task = previewTasks.find((item) => item.definition.id === taskId)?.definition;
  if (!task || task.version !== taskVersion) {
    throw Object.assign(new Error('找不到指定运维任务或版本。'), { code: 'validation' });
  }
  return task;
}

function createOperationPreview(
  serverId: string,
  request: OperationPreflightRequest,
): OperationPreview {
  const task = operationTask(request.taskId, request.taskVersion);
  const implementation = task.implementations[0];
  const now = Date.now();
  previewOperationCounter += 1;
  const previewId = `preview-operation-${previewOperationCounter}`;
  const steps = implementation.executionSteps.map((step, stepIndex) => ({
    runId: previewId,
    phase: 'execute' as const,
    stepIndex,
    stepId: step.id,
    title: step.title,
    status: 'pending' as const,
    executionId: null,
    outputSummary: null,
    errorMessage: null,
    startedAt: null,
    finishedAt: null,
  }));
  previewOperations.set(previewId, {
    run: {
      id: previewId,
      serverId,
      taskId: task.id,
      taskVersion: task.version,
      riskLevel: task.riskLevel,
      status: 'preview_ready',
      parametersSummary: Object.keys(request.parameters).length
        ? JSON.stringify(request.parameters)
        : null,
      result: null,
      errorCategory: null,
      errorMessage: null,
      createdAt: now,
      updatedAt: now,
      finishedAt: null,
    },
    steps,
  });
  return {
    previewId,
    serverId,
    taskId: task.id,
    taskVersion: task.version,
    implementationId: implementation.id,
    riskLevel: task.riskLevel,
    privilege: task.privilege,
    scope: task.scope,
    status: 'preview_ready',
    stepTitles: [
      ...implementation.preflightSteps.map((step) => step.title),
      ...implementation.executionSteps.map((step) => step.title),
    ],
    estimatedSeconds: task.estimatedSeconds,
  };
}

function requirePreviewOperation(runId: string) {
  const details = previewOperations.get(runId);
  if (!details) throw Object.assign(new Error('找不到运维运行。'), { code: 'validation' });
  return details;
}

type StoredWorkflow = {
  createdAt: number;
  updatedAt: number;
  versions: WorkflowDefinition[];
};

type WithoutEventTiming<T> = T extends unknown ? Omit<T, 'sequence' | 'emittedAt'> : never;
type WorkflowEventInput = WithoutEventTiming<WorkflowEvent>;

let previewWorkflowCounter = 0;
let previewRunCounter = 0;
let previewWorkflows = new Map<string, StoredWorkflow>();
let previewWorkflowRuns = new Map<string, WorkflowRunDetails>();

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function workflowPayload(draft: WorkflowDraft | WorkflowDefinition) {
  return JSON.stringify({
    name: draft.name.trim(),
    description: draft.description,
    nodes: draft.nodes,
    edges: draft.edges,
  });
}

function previewChecksum(payload: string) {
  let hash = 2166136261;
  for (let index = 0; index < payload.length; index += 1) {
    hash ^= payload.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return Math.abs(hash >>> 0).toString(16).padStart(8, '0').repeat(8);
}

function workflowError(code: string, message: string) {
  return Object.assign(new Error(message), { code });
}

function validatePreviewWorkflow(draft: WorkflowDraft): WorkflowValidationReport {
  const diagnostics: WorkflowDiagnostic[] = [];
  const nodeById = new Map<string, WorkflowNode>();
  const duplicateIds = new Set<string>();

  if (!draft.name.trim() || draft.nodes.length > 100 || draft.edges.length > 200) {
    diagnostics.push({
      code: draft.nodes.length > 100 || draft.edges.length > 200 ? 'graph_limit' : 'invalid_parameters',
      nodeId: null,
      message: !draft.name.trim() ? '工作流名称不能为空。' : '预览工作流最多包含 100 个节点和 200 条连接。',
    });
  }

  for (const node of draft.nodes) {
    if (nodeById.has(node.id)) duplicateIds.add(node.id);
    nodeById.set(node.id, node);
    if (!node.name.trim()) {
      diagnostics.push({ code: 'invalid_parameters', nodeId: node.id, message: '节点名称不能为空。' });
    }
  }
  for (const nodeId of duplicateIds) {
    diagnostics.push({ code: 'duplicate_node', nodeId, message: '节点 ID 重复。' });
  }

  const starts = draft.nodes.filter((node) => node.config.type === 'start');
  if (starts.length !== 1) {
    diagnostics.push({ code: 'start_count', nodeId: null, message: '必须且只能有一个开始节点。' });
  }

  const outgoing = new Map<string, typeof draft.edges>();
  const incomingCount = new Map<string, number>();
  const edgeKeys = new Set<string>();
  for (const edge of draft.edges) {
    if (!nodeById.has(edge.from) || !nodeById.has(edge.to)) {
      diagnostics.push({ code: 'missing_node', nodeId: null, message: '连接引用了不存在的节点。' });
      continue;
    }
    if (edge.from === edge.to) {
      diagnostics.push({ code: 'self_edge', nodeId: edge.from, message: '节点不能连接到自身。' });
    }
    const edgeKey = `${edge.from}:${edge.to}:${edge.branch}`;
    if (edgeKeys.has(edgeKey)) {
      diagnostics.push({ code: 'duplicate_edge', nodeId: edge.from, message: '存在重复连接。' });
    }
    edgeKeys.add(edgeKey);
    outgoing.set(edge.from, [...(outgoing.get(edge.from) ?? []), edge]);
    incomingCount.set(edge.to, (incomingCount.get(edge.to) ?? 0) + 1);
  }

  for (const node of draft.nodes) {
    const edges = outgoing.get(node.id) ?? [];
    if (node.config.type === 'start' && edges.length !== 1) {
      diagnostics.push({ code: 'start_edges', nodeId: node.id, message: '开始节点必须有一条后续连接。' });
    } else if (node.config.type === 'stop' && edges.length > 0) {
      diagnostics.push({ code: 'stop_edges', nodeId: node.id, message: '停止节点不能有后续连接。' });
    } else if (node.config.type === 'condition') {
      const branches = new Set(edges.map((edge) => edge.branch));
      if (edges.length !== 2 || !branches.has('true') || !branches.has('false')) {
        diagnostics.push({
          code: 'condition_branches',
          nodeId: node.id,
          message: '条件节点必须同时连接 true 和 false 分支。',
        });
      }
    } else if (node.config.type !== 'stop' && edges.some((edge) => edge.branch !== 'success')) {
      diagnostics.push({ code: 'invalid_branch', nodeId: node.id, message: '普通节点只能使用 success 连接。' });
    }
    if (node.config.type !== 'start' && (incomingCount.get(node.id) ?? 0) === 0) {
      diagnostics.push({ code: 'unreachable_node', nodeId: node.id, message: '节点不可到达。' });
    }
  }

  const startNodeId = starts.length === 1 ? starts[0].id : null;
  if (startNodeId) {
    const visiting = new Set<string>();
    const visited = new Set<string>();
    let hasCycle = false;
    const visit = (nodeId: string) => {
      if (visiting.has(nodeId)) {
        hasCycle = true;
        return;
      }
      if (visited.has(nodeId)) return;
      visiting.add(nodeId);
      for (const edge of outgoing.get(nodeId) ?? []) visit(edge.to);
      visiting.delete(nodeId);
      visited.add(nodeId);
    };
    visit(startNodeId);
    if (hasCycle) diagnostics.push({ code: 'cycle', nodeId: null, message: '工作流不能包含环。' });
    for (const node of draft.nodes) {
      if (!visited.has(node.id)) {
        diagnostics.push({ code: 'unreachable_node', nodeId: node.id, message: '节点无法从开始节点到达。' });
      }
    }
  }

  return { valid: diagnostics.length === 0, startNodeId, diagnostics };
}

function getStoredWorkflow(workflowId: string) {
  const stored = previewWorkflows.get(workflowId);
  if (!stored) throw workflowError('not_found', '找不到指定工作流。');
  return stored;
}

function findWorkflow(workflowId: string, version: number | null) {
  const stored = getStoredWorkflow(workflowId);
  const definition = version === null
    ? stored.versions.at(-1)
    : stored.versions.find((candidate) => candidate.version === version);
  if (!definition) throw workflowError('not_found', '找不到指定工作流版本。');
  return definition;
}

function createNodeRun(runId: string, nodeId: string, attempt: number): WorkflowNodeRun {
  return {
    runId,
    nodeId,
    attempt,
    status: 'pending',
    executionId: null,
    startedAt: null,
    finishedAt: null,
    durationMs: null,
    exitCode: null,
    result: null,
    outputSummary: null,
    errorMessage: null,
    retryable: false,
  };
}

function appendWorkflowEvent(
  details: WorkflowRunDetails,
  event: WorkflowEventInput,
  onEvent?: (event: WorkflowEvent) => void,
) {
  const fullEvent = {
    ...event,
    sequence: details.events.length + 1,
    emittedAt: Date.now() + details.events.length,
  } as WorkflowEvent;
  details.events.push({
    runId: details.run.id,
    sequence: fullEvent.sequence,
    eventType: fullEvent.type,
    payload: clone(fullEvent) as unknown as Record<string, unknown>,
    emittedAt: fullEvent.emittedAt,
  });
  onEvent?.(clone(fullEvent));
}

function updateRunStatus(
  details: WorkflowRunDetails,
  status: WorkflowRunStatus,
  message: string | null,
  onEvent?: (event: WorkflowEvent) => void,
) {
  details.run.status = status;
  details.run.errorMessage = message;
  appendWorkflowEvent(details, {
    type: 'runStatusChanged',
    runId: details.run.id,
    status,
    message,
  }, onEvent);
}

function finishNode(
  details: WorkflowRunDetails,
  nodeRun: WorkflowNodeRun,
  status: WorkflowNodeRun['status'],
  onEvent?: (event: WorkflowEvent) => void,
  message: string | null = null,
) {
  const now = Date.now();
  nodeRun.status = status;
  nodeRun.finishedAt = now;
  nodeRun.durationMs = nodeRun.startedAt === null ? 0 : Math.max(1, now - nodeRun.startedAt);
  nodeRun.errorMessage = message;
  appendWorkflowEvent(details, {
    type: 'nodeStatusChanged',
    runId: details.run.id,
    nodeId: nodeRun.nodeId,
    attempt: nodeRun.attempt,
    status,
    executionId: nodeRun.executionId,
    message,
  }, onEvent);
}

function completePreviewWorkflow(
  details: WorkflowRunDetails,
  definition: WorkflowDefinition,
  onEvent: (event: WorkflowEvent) => void,
  options: { retrying?: boolean } = {},
) {
  const outgoing = new Map<string, typeof definition.edges>();
  for (const edge of definition.edges) {
    outgoing.set(edge.from, [...(outgoing.get(edge.from) ?? []), edge]);
  }

  const existingDone = new Set(
    details.nodeRuns.filter((run) => run.status === 'succeeded').map((run) => run.nodeId),
  );
  let currentId = options.retrying
    ? details.run.currentNodeId
    : definition.nodes.find((node) => node.config.type === 'start')?.id ?? null;
  let guard = 0;
  while (currentId && guard < definition.nodes.length + 2) {
    guard += 1;
    const node = definition.nodes.find((candidate) => candidate.id === currentId);
    if (!node) break;
    let nodeRun = details.nodeRuns
      .filter((candidate) => candidate.nodeId === node.id)
      .sort((left, right) => right.attempt - left.attempt)[0];
    if (!nodeRun || (options.retrying && nodeRun.status === 'failed')) {
      nodeRun = createNodeRun(details.run.id, node.id, (nodeRun?.attempt ?? 0) + 1);
      details.nodeRuns.push(nodeRun);
    }
    if (!existingDone.has(node.id) || options.retrying) {
      nodeRun.status = 'running';
      nodeRun.startedAt = Date.now();
      nodeRun.executionId = ['task', 'custom', 'upload', 'download', 'logSearch'].includes(node.config.type)
        ? `preview-execution-${details.nodeRuns.length}`
        : null;
      details.run.currentNodeId = node.id;
      appendWorkflowEvent(details, {
        type: 'nodeStarted', runId: details.run.id, nodeId: node.id, attempt: nodeRun.attempt,
      }, onEvent);

      if (node.config.type === 'task' && node.config.taskId === 'preview.fail' && !options.retrying) {
        nodeRun.retryable = true;
        details.run.retryable = true;
        details.run.errorCategory = 'preview_injected_failure';
        finishNode(details, nodeRun, 'failed', onEvent, 'Preview 注入失败：可从此节点重试。');
        updateRunStatus(details, 'paused', '节点失败，工作流已暂停。', onEvent);
        return;
      }

      if (node.config.type === 'condition') {
        const conditionConfig = node.config;
        const source = definition.nodes.find((candidate) => candidate.id === conditionConfig.sourceNodeId);
        const result = source?.config.type === 'task'
          ? source.config.parameters.previewCondition !== false
          : true;
        nodeRun.result = { matched: result };
        appendWorkflowEvent(details, {
          type: 'conditionEvaluated', runId: details.run.id, nodeId: node.id, result,
        }, onEvent);
      } else {
        nodeRun.result = { preview: true };
      }
      nodeRun.exitCode = 0;
      nodeRun.outputSummary = 'Preview 内存执行成功';
      finishNode(details, nodeRun, 'succeeded', onEvent);
    }

    const edges = outgoing.get(node.id) ?? [];
    if (node.config.type === 'condition') {
      const result = (nodeRun.result as { matched?: boolean } | null)?.matched ?? true;
      const selected = edges.find((edge) => edge.branch === (result ? 'true' : 'false'));
      const skipped = edges.find((edge) => edge.branch === (result ? 'false' : 'true'));
      if (skipped && !details.nodeRuns.some((run) => run.nodeId === skipped.to)) {
        const skippedRun = createNodeRun(details.run.id, skipped.to, 1);
        details.nodeRuns.push(skippedRun);
        finishNode(details, skippedRun, 'skipped', onEvent, '条件分支未选中。');
      }
      currentId = selected?.to ?? null;
    } else {
      currentId = edges.find((edge) => edge.branch === 'success')?.to ?? null;
    }
    options.retrying = false;
  }

  const finishedAt = Date.now();
  details.run.currentNodeId = null;
  details.run.finishedAt = finishedAt;
  details.run.durationMs = Math.max(1, finishedAt - (details.run.startedAt ?? finishedAt));
  details.run.retryable = false;
  details.run.errorCategory = null;
  updateRunStatus(details, 'succeeded', null, onEvent);
  appendWorkflowEvent(details, {
    type: 'finished', runId: details.run.id, status: 'succeeded', durationMs: details.run.durationMs,
  }, onEvent);
}

export function resetWorkflowPreviewForTests() {
  previewWorkflowCounter = 0;
  previewRunCounter = 0;
  previewWorkflows = new Map();
  previewWorkflowRuns = new Map();
}

const workflowPreviewApi = {
  listWorkflows: async (): Promise<WorkflowSummary[]> =>
    [...previewWorkflows.entries()].map(([id, stored]) => {
      const current = stored.versions.at(-1)!;
      return {
        id,
        name: current.name,
        description: current.description,
        currentVersion: current.version,
        createdAt: stored.createdAt,
        updatedAt: stored.updatedAt,
      };
    }).sort((left, right) => right.updatedAt - left.updatedAt),
  getWorkflow: async (workflowId: string, version: number | null): Promise<WorkflowDefinition | null> => {
    const stored = previewWorkflows.get(workflowId);
    if (!stored) return null;
    const definition = version === null
      ? stored.versions.at(-1)
      : stored.versions.find((candidate) => candidate.version === version);
    return definition ? clone(definition) : null;
  },
  saveWorkflow: async (draft: WorkflowDraft): Promise<WorkflowDefinition> => {
    const report = validatePreviewWorkflow(draft);
    if (!report.valid) throw workflowError('validation', report.diagnostics[0].message);
    const now = Date.now();
    const id = draft.id ?? `preview-workflow-${++previewWorkflowCounter}`;
    const existing = previewWorkflows.get(id);
    const payload = workflowPayload(draft);
    const latest = existing?.versions.at(-1);
    if (latest && workflowPayload(latest) === payload) return clone(latest);
    const definition: WorkflowDefinition = {
      id,
      name: draft.name.trim(),
      description: draft.description,
      nodes: clone(draft.nodes),
      edges: clone(draft.edges),
      version: (latest?.version ?? 0) + 1,
      checksumSha256: previewChecksum(payload),
    };
    previewWorkflows.set(id, {
      createdAt: existing?.createdAt ?? now,
      updatedAt: now,
      versions: [...(existing?.versions ?? []), definition],
    });
    return clone(definition);
  },
  deleteWorkflow: async (workflowId: string) => previewWorkflows.delete(workflowId),
  validateWorkflow: async (draft: WorkflowDraft) => validatePreviewWorkflow(draft),
  startWorkflowRun: async (
    request: StartWorkflowRunRequest,
    onEvent: (event: WorkflowEvent) => void,
  ): Promise<WorkflowRunDetails> => {
    const definition = findWorkflow(request.workflowId, request.workflowVersion);
    const now = Date.now();
    const runId = `preview-run-${++previewRunCounter}`;
    const run: WorkflowRunRecord = {
      id: runId,
      workflowId: definition.id,
      workflowVersion: definition.version,
      serverId: request.serverId,
      status: 'running',
      currentNodeId: null,
      createdAt: now,
      startedAt: now,
      finishedAt: null,
      durationMs: null,
      errorCategory: null,
      errorMessage: null,
      retryable: false,
    };
    const details: WorkflowRunDetails = { run, nodeRuns: [], restorePoints: [], events: [] };
    previewWorkflowRuns.set(runId, details);
    appendWorkflowEvent(details, {
      type: 'runStarted', runId, workflowId: definition.id, serverId: request.serverId,
    }, onEvent);
    if (definition.name.includes('取消演示')) {
      updateRunStatus(details, 'running', 'Preview 长任务正在运行，可执行取消。', onEvent);
      return clone(details);
    }
    completePreviewWorkflow(details, definition, onEvent);
    return clone(details);
  },
  cancelWorkflowRun: async (runId: string) => {
    const details = previewWorkflowRuns.get(runId);
    if (!details) throw workflowError('not_found', '找不到指定运行。');
    if (details.run.status !== 'running') throw workflowError('invalid_state', '只有运行中的工作流可以取消。');
    details.run.finishedAt = Date.now();
    details.run.durationMs = Math.max(1, details.run.finishedAt - (details.run.startedAt ?? details.run.finishedAt));
    updateRunStatus(details, 'cancelled', 'Preview 运行已取消。');
    appendWorkflowEvent(details, {
      type: 'finished', runId, status: 'cancelled', durationMs: details.run.durationMs,
    });
  },
  retryWorkflowNode: async (
    runId: string,
    _dangerousConfirmed: boolean,
    onEvent: (event: WorkflowEvent) => void,
  ): Promise<WorkflowRunDetails> => {
    const details = previewWorkflowRuns.get(runId);
    if (!details) throw workflowError('not_found', '找不到指定运行。');
    if (details.run.status !== 'paused' || !details.run.currentNodeId) {
      throw workflowError('invalid_state', '只有暂停且可重试的工作流可以重试。');
    }
    const definition = findWorkflow(details.run.workflowId, details.run.workflowVersion);
    updateRunStatus(details, 'running', '正在从失败节点重试。', onEvent);
    completePreviewWorkflow(details, definition, onEvent, { retrying: true });
    return clone(details);
  },
  listWorkflowRuns: async (filter: WorkflowRunFilter): Promise<WorkflowRunRecord[]> =>
    [...previewWorkflowRuns.values()]
      .map((details) => details.run)
      .filter((run) => !filter.workflowId || run.workflowId === filter.workflowId)
      .filter((run) => !filter.serverId || run.serverId === filter.serverId)
      .filter((run) => !filter.status || run.status === filter.status)
      .filter((run) => !filter.createdFrom || run.createdAt >= filter.createdFrom)
      .filter((run) => !filter.createdTo || run.createdAt <= filter.createdTo)
      .sort((left, right) => right.createdAt - left.createdAt)
      .map(clone),
  getWorkflowRun: async (runId: string) => {
    const details = previewWorkflowRuns.get(runId);
    return details ? clone(details) : null;
  },
  rollbackWorkflowRun: async (runId: string, dangerousConfirmed: boolean) => {
    if (!dangerousConfirmed) throw workflowError('confirmation_required', '回滚前必须确认。');
    const details = previewWorkflowRuns.get(runId);
    if (!details) throw workflowError('not_found', '找不到指定运行。');
    for (const point of [...details.restorePoints].reverse()) {
      if (point.status === 'available') point.status = 'rolled_back';
    }
    details.run.status = 'rolled_back';
    details.run.finishedAt = Date.now();
    details.run.errorMessage = null;
    appendWorkflowEvent(details, {
      type: 'runStatusChanged', runId, status: 'rolled_back', message: 'Preview 回滚完成。',
    });
    return clone(details);
  },
  cleanupWorkflowRestorePoints: async (runId: string) => {
    const details = previewWorkflowRuns.get(runId);
    if (!details) throw workflowError('not_found', '找不到指定运行。');
    let cleaned = 0;
    for (const point of details.restorePoints) {
      if (point.status !== 'expired') {
        point.status = 'expired';
        cleaned += 1;
      }
    }
    return cleaned;
  },
  exportWorkflowDiagnostics: async (runId: string): Promise<ExecutionFile> => {
    if (!previewWorkflowRuns.has(runId)) throw workflowError('not_found', '找不到指定运行。');
    return {
      id: `preview-diagnostics-${runId}`,
      relativePath: `downloads/${runId}-diagnostics.preview.json`,
      purpose: 'workflow_diagnostics_preview',
      sizeBytes: 0,
      sha256: previewChecksum(runId),
    };
  },
};

export type UpdatePreviewScenario = 'github' | 'modelscope' | 'reject' | 'up_to_date';

function updateScenarioFromUrl(): UpdatePreviewScenario {
  if (typeof window === 'undefined') return 'github';
  const requested = new URLSearchParams(window.location.search).get('update');
  return requested === 'modelscope' || requested === 'reject' || requested === 'up_to_date'
    ? requested
    : 'github';
}

function emptyUpdateStatus(autoCheck = true): UpdateStatus {
  return {
    currentVersion: '0.1.0',
    phase: 'idle',
    autoCheck,
    lastCheckedAt: null,
    lastResult: null,
    release: null,
    fallbackReason: null,
    staged: null,
    lastError: null,
  };
}

let previewUpdateScenario: UpdatePreviewScenario = updateScenarioFromUrl();
let previewUpdateStatus = emptyUpdateStatus();

export function resetUpdatePreviewForTests(scenario: UpdatePreviewScenario = 'github') {
  previewUpdateScenario = scenario;
  previewUpdateStatus = emptyUpdateStatus();
}

function previewUpdateError(message: string) {
  return { code: 'update', message };
}

function previewRelease(source: UpdateSource) {
  return {
    version: '0.2.0',
    notes: '增强国产 Linux 自动识别、日志检索下载和更新安全校验。',
    publishedAt: '2026-08-04T10:00:00Z',
    size: 24 * 1024 * 1024,
    buildId: 'preview-20260804',
    source,
    sourceLabel: source === 'github' ? 'GitHub Releases' : 'ModelScope 国内镜像',
  };
}

const updatePreviewApi = {
  getUpdateStatus: async (): Promise<UpdateStatus> => clone(previewUpdateStatus),
  setAutoUpdateCheck: async (enabled: boolean): Promise<UpdateStatus> => {
    previewUpdateStatus.autoCheck = enabled;
    return clone(previewUpdateStatus);
  },
  checkForUpdate: async (manual: boolean): Promise<UpdateStatus> => {
    if (!manual && !previewUpdateStatus.autoCheck) return clone(previewUpdateStatus);
    const checkedAt = Math.floor(Date.now() / 1000);
    previewUpdateStatus.lastCheckedAt = checkedAt;
    previewUpdateStatus.lastError = null;
    previewUpdateStatus.staged = null;
    if (previewUpdateScenario === 'up_to_date') {
      previewUpdateStatus = {
        ...previewUpdateStatus,
        phase: 'up_to_date',
        release: null,
        fallbackReason: null,
        lastResult: {
          status: 'up_to_date',
          version: previewUpdateStatus.currentVersion,
          source: 'github',
          message: '当前已是最新版本',
        },
      };
      return clone(previewUpdateStatus);
    }
    const source: UpdateSource = previewUpdateScenario === 'modelscope' ? 'modelscope' : 'github';
    previewUpdateStatus = {
      ...previewUpdateStatus,
      phase: 'available',
      release: previewRelease(source),
      fallbackReason: source === 'modelscope' ? 'GitHub 暂时不可用，已切换国内镜像。' : null,
      lastResult: {
        status: 'available',
        version: '0.2.0',
        source,
        message: '发现可用更新',
      },
    };
    return clone(previewUpdateStatus);
  },
  downloadUpdate: async (
    onEvent: (event: UpdateProgressEvent) => void,
  ): Promise<UpdateStatus> => {
    if (previewUpdateStatus.phase !== 'available' || !previewUpdateStatus.release) {
      throw previewUpdateError('当前更新状态不允许下载。');
    }
    previewUpdateStatus.phase = 'downloading';
    const total = previewUpdateStatus.release.size;
    onEvent({ sequence: 1, downloadedBytes: Math.floor(total * 0.2), totalBytes: total });
    onEvent({ sequence: 2, downloadedBytes: Math.floor(total * 0.7), totalBytes: total });
    onEvent({ sequence: 3, downloadedBytes: total, totalBytes: total });
    if (previewUpdateScenario === 'reject') {
      previewUpdateStatus.phase = 'failed';
      previewUpdateStatus.lastError = '更新签名验证失败，文件已拒绝并清理。';
      throw previewUpdateError(previewUpdateStatus.lastError);
    }
    previewUpdateStatus.phase = 'downloaded';
    previewUpdateStatus.staged = {
      version: previewUpdateStatus.release.version,
      relativePath: 'staged/0.2.0/QingzhouSSH-0.2.0-windows-x86_64.nsis',
      sha256: 'a'.repeat(64),
      size: total,
    };
    return clone(previewUpdateStatus);
  },
  installUpdate: async (confirmed: boolean): Promise<UpdateStatus> => {
    if (!confirmed) throw previewUpdateError('安装更新前必须明确确认。');
    if (previewUpdateStatus.phase !== 'downloaded') {
      throw previewUpdateError('更新包尚未完成下载。');
    }
    previewUpdateStatus.phase = 'installing';
    return clone(previewUpdateStatus);
  },
  clearDownloadedUpdate: async (): Promise<UpdateStatus> => {
    previewUpdateStatus = {
      ...emptyUpdateStatus(previewUpdateStatus.autoCheck),
      lastCheckedAt: previewUpdateStatus.lastCheckedAt,
      lastResult: previewUpdateStatus.lastResult,
    };
    return clone(previewUpdateStatus);
  },
};

export const previewApi = {
  ...updatePreviewApi,
  ...workflowPreviewApi,
  bootstrapStatus: async () => ({
    state: 'ready' as const,
    dataRoot: previewDataRoot,
  }),
  initializeDataRoot: async (path: string) => ({ state: 'ready' as const, dataRoot: path }),
  listServers: async () => previewServers,
  createServer: async (request: CreateServerRequest) => {
    const profile: ServerProfile = {
      id: `preview-${previewServers.length + 1}`,
      name: request.name,
      host: request.host,
      port: request.port,
      username: request.username,
      authKind: request.credential.kind,
      credentialId: `preview-credential-${previewServers.length + 1}`,
    };
    previewServers = [...previewServers, profile];
    return profile;
  },
  inspectHostKey: async (_serverId: string): Promise<HostKeyCheck> => ({
    decision: 'trusted',
    observed: {
      algorithm: 'ssh-ed25519',
      fingerprintSha256: 'SHA256:QingzhouPreviewVerifiedHostKey',
      rawKeyBase64: 'preview-only',
    },
    trusted: {
      serverId: previewServer.id,
      algorithm: 'ssh-ed25519',
      fingerprintSha256: 'SHA256:QingzhouPreviewVerifiedHostKey',
      rawKeyBase64: 'preview-only',
    },
  }),
  trustHostKey: async (_serverId: string, _observation: HostKeyObservation) => undefined,
  testConnection: async (_serverId: string): Promise<SystemCapabilities> => ({
    osId: 'openEuler',
    osFamily: 'RHEL / 国产 Linux',
    versionId: '24.03 LTS',
    packageManager: 'dnf',
    serviceManager: 'systemd',
    architecture: 'x86_64',
    shell: '/bin/bash',
    commands: ['grep', 'gzip', 'awk', 'systemctl', 'ps', 'df', 'sh'],
  }),
  listTaskDefinitions: async (_serverId: string) => previewTasks,
  listOperationsTasks: async (_serverId: string) => previewTasks,
  preflightOperation: async (serverId: string, request: OperationPreflightRequest) =>
    createOperationPreview(serverId, request),
  startOperation: async (
    serverId: string,
    request: OperationStartRequest,
    onEvent: (event: ExecutionEvent) => void,
  ): Promise<OperationRunDetails> => {
    const task = operationTask(request.taskId, request.taskVersion);
    if (task.riskLevel === 'dangerous' && !request.confirmedPreviewId) {
      throw Object.assign(new Error('危险任务必须先预览并确认。'), { code: 'validation' });
    }
    const previewId = request.confirmedPreviewId
      ?? createOperationPreview(serverId, request).previewId;
    const details = requirePreviewOperation(previewId);
    if (
      details.run.serverId !== serverId
      || details.run.taskId !== request.taskId
      || details.run.taskVersion !== request.taskVersion
    ) {
      throw Object.assign(new Error('确认的预览与任务不一致。'), { code: 'validation' });
    }
    if (task.riskLevel === 'dangerous') {
      details.run.status = 'waiting_confirmation';
      details.run.updatedAt = Date.now();
      return clone(details);
    }

    const execution = createPreviewExecution(serverId, task.id);
    emitPreview(onEvent, execution);
    const finishedAt = Date.now();
    details.run.status = 'succeeded';
    details.run.updatedAt = finishedAt;
    details.run.finishedAt = finishedAt;
    details.steps = details.steps.map((step) => ({
      ...step,
      status: 'succeeded',
      executionId: execution.record.id,
      outputSummary: execution.record.outputSummary,
      startedAt: finishedAt - 100,
      finishedAt,
    }));
    return clone(details);
  },
  cancelOperation: async (runId: string) => {
    const details = requirePreviewOperation(runId);
    details.run.status = 'cancelled';
    details.run.updatedAt = Date.now();
    details.run.finishedAt = details.run.updatedAt;
  },
  getOperation: async (runId: string) => {
    const details = previewOperations.get(runId);
    return details ? clone(details) : null;
  },
  listOperations: async (filter: OperationFilter): Promise<OperationRunRecord[]> =>
    [...previewOperations.values()]
      .map((details) => details.run)
      .filter((run) => !filter.serverId || run.serverId === filter.serverId)
      .filter((run) => !filter.taskId || run.taskId === filter.taskId)
      .filter((run) => !filter.status || run.status === filter.status)
      .map(clone),
  startTaskExecution: async (
    serverId: string,
    request: TaskExecutionRequest,
    onEvent: (event: ExecutionEvent) => void,
  ) => {
    const details = createPreviewExecution(serverId, request.taskId);
    emitPreview(onEvent, details);
    return details;
  },
  startCustomExecution: async (
    serverId: string,
    request: CustomExecutionRequest,
    onEvent: (event: ExecutionEvent) => void,
  ) => {
    const details = createPreviewExecution(serverId, `advanced.${request.mode}`);
    emitPreview(onEvent, details);
    return details;
  },
  cancelExecution: async (_executionId: string) => undefined,
  listLocalDirectory: async (path: string | null): Promise<DirectoryListing> => ({
    path: path ?? previewDataRoot,
    parent: null,
    entries: [
      { name: 'downloads', path: `${previewDataRoot}\\downloads`, kind: 'directory', size: null, modifiedAt: null },
      { name: 'example.log', path: `${previewDataRoot}\\example.log`, kind: 'file', size: 4096, modifiedAt: 1_775_000_000 },
    ],
  }),
  listRemoteDirectory: async (_serverId: string, path: string): Promise<DirectoryListing> => ({
    path,
    parent: path === '/' ? null : '/',
    entries: path === '/'
      ? [
          { name: 'home', path: '/home', kind: 'directory', size: null, modifiedAt: null },
          { name: 'var', path: '/var', kind: 'directory', size: null, modifiedAt: null },
        ]
      : [
          { name: 'app.log', path: `${path}/app.log`.replace('//', '/'), kind: 'file', size: 8192, modifiedAt: 1_775_000_000 },
        ],
  }),
  searchLogs: async (
    serverId: string,
    request: LogSearchRequest,
    onEvent: (event: ExecutionEvent) => void,
  ) => {
    previewLogTarget = request.target;
    const details = createPreviewExecution(serverId, 'logs.search');
    emitPreview(onEvent, details);
    return details;
  },
  readLogResultPage: async (_executionId: string, cursor: string | null, pageSize: number) => {
    if (previewLogTarget === 'filename') {
      return {
        items: [
          {
            resultType: 'file' as const,
            path: '/home/app/requirements.txt',
            name: 'requirements.txt',
            size: 96,
            modifiedAt: 1_785_801_600,
          },
        ],
        nextCursor: null,
      };
    }
    const start = cursor ? Number(cursor) : 0;
    const items = Array.from({ length: Math.min(pageSize, 8) }, (_, index) => ({
      resultType: 'content' as const,
      path: '/var/log/app.log',
      lineNumber: start + index + 1,
      kind: 'match' as const,
      timestamp: '2026-08-03',
      text: `预览日志匹配 ${start + index + 1}`,
    }));
    return { items, nextCursor: null };
  },
  downloadLogResult: async (_executionId: string, suggestedName: string) =>
    `downloads/${suggestedName}`,
  uploadFile: async (
    serverId: string,
    _request: UploadRequest,
    onEvent: (event: ExecutionEvent) => void,
  ) => {
    const details = createPreviewExecution(serverId, 'transfer.upload');
    emitPreview(onEvent, details);
    return details;
  },
  downloadFile: async (
    serverId: string,
    request: DownloadRequest,
    onEvent: (event: ExecutionEvent) => void,
  ) => {
    const details = createPreviewExecution(serverId, 'transfer.download');
    details.files.push({
      id: 'preview-file',
      relativePath: `downloads/${request.suggestedName}`,
      purpose: 'download',
      sizeBytes: 1024,
      sha256: 'a'.repeat(64),
    });
    emitPreview(onEvent, details);
    return details;
  },
  listExecutions: async (filter: ExecutionFilter) =>
    previewExecutions
      .map((details) => details.record)
      .filter((record) => !filter.status || record.status === filter.status),
  getExecution: async (executionId: string) =>
    previewExecutions.find((details) => details.record.id === executionId) ?? null,
};

export const dataRootPreviewApi = {
  ...previewApi,
  bootstrapStatus: async () => ({ state: 'needs_selection' as const }),
};
