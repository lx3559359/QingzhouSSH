import type {
  CreateServerRequest,
  ConfirmTaskRemediationRequest,
  CustomExecutionRequest,
  DataMigrationJournal,
  DataMigrationPreview,
  DownloadRequest,
  DirectoryListing,
  ExecutionDetails,
  ExecutionEvent,
  ExecutionFile,
  ExecutionFilter,
  LogSearchRequest,
  OperationFilter,
  OperationConfirmRequest,
  OperationBatchDetails,
  OperationBatchRequest,
  ReportFormat,
  OperationPreflightRequest,
  OperationPreview,
  OperationRunDetails,
  OperationRunRecord,
  OperationRecoveryResult,
  OperationRestoreDetails,
  OperationStartRequest,
  HostKeyCheck,
  HostKeyObservation,
  ServerProfile,
  SystemCapabilities,
  TaskAvailability,
  TaskRemediationPreview,
  TaskExecutionRequest,
  UploadRequest,
  TransferJob,
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
  ConfirmPersonalScriptRunRequest,
  CreatePersonalScriptRequest,
  PersonalScriptDetails,
  PersonalScriptListFilter,
  PersonalScriptRunPreview,
  PersonalScriptRunResult,
  PersonalScriptSummary,
  PersonalScriptVersion,
  SavePersonalScriptVersionRequest,
  ScriptPackageExport,
  UpdatePersonalScriptMetadataRequest,
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
let previewTransferJobs: TransferJob[] = [];
let previewPersonalScripts = new Map<string, PersonalScriptDetails>();
let previewPersonalScriptVersions = new Map<string, PersonalScriptVersion[]>();
let previewPersonalScriptRuns = new Map<string, PersonalScriptRunPreview>();
let previewLogTarget: LogSearchRequest['target'] = 'content';
let previewLogKeyword = '';
let previewLogCaseSensitive = false;
let previewDataMigration: DataMigrationJournal | null = null;
let previewDataMigrationTarget: string | null = null;
const previewDataRoot =
  import.meta.env.VITE_QINGZHOU_DATA_ROOT ?? '.local\\dev-data（项目目录内）';

const previewTasks: TaskAvailability[] = [
  {
    state: 'ready',
    summary: '当前服务器可以直接运行',
    missingCommands: [],
    remediation: null,
    library: {
      source: 'reviewed_command',
      primaryCategory: 'daily_inspection',
      keywords: ['系统概览', '日常巡检'],
      noviceAliases: ['检查服务器'],
    },
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
    state: 'ready',
    summary: '当前服务器可以直接运行',
    missingCommands: [],
    remediation: null,
    library: {
      source: 'builtin_task',
      primaryCategory: 'service_management',
      keywords: ['重启服务', '服务管理'],
      noviceAliases: ['服务没有响应'],
    },
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

previewTasks.push(
  {
    state: 'remediable',
    summary: '服务器缺少 tcpdump，可以在确认后补齐白名单组件',
    missingCommands: ['tcpdump'],
    remediation: {
      packageManager: 'dnf',
      missingCommands: ['tcpdump'],
      packages: ['tcpdump'],
    },
    library: {
      source: 'reviewed_command',
      primaryCategory: 'network',
      keywords: ['抓包', '网络诊断', 'tcpdump'],
      noviceAliases: ['网络断断续续', '查谁在访问服务器'],
    },
    definition: {
      ...previewTasks[0]!.definition,
      id: 'network.packet_capture',
      category: 'network',
      title: '限时抓包摘要',
      description: '抓取限定数量的数据包并生成摘要',
      outputKind: 'text',
      implementations: [{
        ...previewTasks[0]!.definition.implementations[0]!,
        id: 'linux-tcpdump',
        compatibility: {
          osFamilies: ['debian', 'rhel', 'openeuler'],
          serviceManagers: [],
          requiredCommands: ['tcpdump'],
        },
      }],
    },
  },
  {
    state: 'permission_blocked',
    summary: '当前账号不是 root，且服务器未配置免密 sudo',
    missingCommands: [],
    remediation: null,
    library: {
      source: 'builtin_task',
      primaryCategory: 'security_login',
      keywords: ['防火墙', '安全规则'],
      noviceAliases: ['端口放不开', '外网访问不了'],
    },
    definition: {
      ...previewTasks[0]!.definition,
      id: 'security.firewall_manage',
      category: 'security',
      title: '管理防火墙规则',
      description: '需要 root 或免密 sudo 权限',
      riskLevel: 'dangerous',
      privilege: 'root_or_passwordless_sudo',
      scope: 'single_server',
    },
  },
  {
    state: 'unsupported',
    summary: '安全回滚与断线重连验证尚未完成，当前版本不会执行',
    missingCommands: [],
    remediation: null,
    library: {
      source: 'builtin_task',
      primaryCategory: 'system_settings',
      keywords: ['IP', '网络地址'],
      noviceAliases: ['修改服务器地址'],
    },
    definition: {
      ...previewTasks[0]!.definition,
      id: 'network.ip_change',
      category: 'network',
      title: '修改 IP 地址',
      description: '涉及断线的高风险网络配置变更',
      riskLevel: 'dangerous',
      privilege: 'root_or_passwordless_sudo',
      scope: 'single_server',
    },
  },
);

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

function createPreviewTransferJob(
  serverId: string,
  direction: 'upload' | 'download',
  sourcePath: string,
  targetPath: string,
  overwrite: boolean,
  verification: UploadRequest['verification'],
): TransferJob {
  const now = Date.now();
  const job: TransferJob = {
    id: `preview-transfer-${previewTransferJobs.length + 1}`,
    executionId: null,
    serverId,
    direction,
    sourcePath,
    targetPath,
    overwrite,
    verification,
    status: 'succeeded',
    transferred: 1024,
    total: 1024,
    percent: 100,
    bytesPerSecond: 4096,
    averageBytesPerSecond: 3072,
    etaSeconds: 0,
    attemptCount: 1,
    maxAttempts: 3,
    cancelRequested: false,
    retryable: false,
    errorCategory: null,
    errorMessage: null,
    sha256: 'a'.repeat(64),
    location: direction === 'download' ? `downloads/${targetPath}` : targetPath,
    createdAt: now,
    updatedAt: now + 360,
    startedAt: now,
    finishedAt: now + 360,
  };
  previewTransferJobs = [job, ...previewTransferJobs];
  return job;
}

function emitTransferPreview(
  onEvent: (event: ExecutionEvent) => void,
  details: ExecutionDetails,
  location: string,
) {
  const startedAt = details.record.startedAt ?? Date.now();
  const progress = (
    sequence: number,
    elapsedMs: number,
    phase: 'connecting' | 'transferring' | 'verifying' | 'finalizing',
    transferred: number,
    bytesPerSecond: number | null,
    etaSeconds: number | null,
  ): ExecutionEvent => ({
    type: 'progress',
    sequence,
    emittedAt: startedAt + elapsedMs,
    phase,
    transferred,
    total: phase === 'connecting' ? null : 1024,
    percent: phase === 'connecting' ? null : transferred / 10.24,
    bytesPerSecond,
    averageBytesPerSecond: bytesPerSecond == null ? null : 2048,
    etaSeconds,
  });
  onEvent({
    type: 'started',
    sequence: 1,
    emittedAt: startedAt,
    executionId: details.record.id,
    startedAt,
  });
  onEvent(progress(2, 20, 'connecting', 0, null, null));
  onEvent(progress(3, 120, 'transferring', 512, 4096, 1));
  onEvent(progress(4, 240, 'transferring', 1024, 4096, 0));
  onEvent(progress(5, 260, 'verifying', 1024, 4096, 0));
  onEvent(progress(6, 280, 'finalizing', 1024, 4096, 0));
  onEvent({
    type: 'finished',
    sequence: 7,
    emittedAt: startedAt + 360,
    status: 'succeeded',
    exitCode: 0,
    durationMs: 360,
    result: {
      bytes: 1024,
      sha256: 'a'.repeat(64),
      location,
      verificationLevel: 'remote_hash',
      remoteHashCompared: true,
    },
  });
}

let previewOperationCounter = 0;
let previewOperations = new Map<string, OperationRunDetails>();
let previewOperationBatches = new Map<string, OperationBatchDetails>();
let previewOperationRestorePoints = new Map<string, OperationRestoreDetails>();

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
  const server = previewServers.find((item) => item.id === serverId) ?? previewServer;
  const dangerous = task.riskLevel === 'dangerous';
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
    confirmationToken: dangerous ? previewId : null,
    server: {
      id: server.id,
      name: server.name,
      host: server.host,
      port: server.port,
      username: server.username,
    },
    permissionSummary: task.privilege === 'current_user'
      ? '使用当前 SSH 用户权限'
      : '执行前必须确认 root 或免密 sudo 权限',
    currentStateSummary: dangerous
      ? '预览模式：已完成只读状态检查'
      : '只读任务将在执行时读取服务器当前状态',
    targetStateSummary: Object.keys(request.parameters).length
      ? JSON.stringify(request.parameters)
      : '此任务没有需要填写的目标参数',
    backupSummary: implementation.backupPlan?.items.map((item) => `执行前备份：${item.id}`) ?? [],
    disconnectRisk: task.id === 'network.ip_change'
      ? {
          mayDisconnect: true,
          explanation: '修改服务器 IP 可能中断当前连接；客户端会先安排远程超时自动恢复',
          automaticRecoverySeconds: Number(request.parameters.rollbackSeconds ?? 120),
        }
      : { mayDisconnect: false, explanation: null, automaticRecoverySeconds: null },
  };
}

function requirePreviewOperation(runId: string) {
  const details = previewOperations.get(runId);
  if (!details) throw Object.assign(new Error('找不到运维运行。'), { code: 'validation' });
  return details;
}

function executePreviewOperation(
  serverId: string,
  request: OperationPreflightRequest,
  confirmationToken: string,
  onEvent: (event: ExecutionEvent) => void,
) {
  const task = operationTask(request.taskId, request.taskVersion);
  const details = requirePreviewOperation(confirmationToken);
  if (
    details.run.serverId !== serverId
    || details.run.taskId !== request.taskId
    || details.run.taskVersion !== request.taskVersion
    || details.run.status !== 'preview_ready'
    || details.run.parametersSummary !== (
      Object.keys(request.parameters).length ? JSON.stringify(request.parameters) : null
    )
  ) {
    throw Object.assign(new Error('确认令牌与本次任务、服务器或参数不一致。'), {
      code: 'validation',
    });
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
  if (task.riskLevel === 'dangerous' && !previewOperationRestorePoints.has(confirmationToken)) {
    previewOperationRestorePoints.set(confirmationToken, {
      point: {
        id: `restore-${confirmationToken}`,
        operationRunId: confirmationToken,
        serverId,
        taskId: task.id,
        status: 'available',
        localRelativeDir: `backups/tasks/${confirmationToken}`,
        remoteAssetId: null,
        expiresAt: null,
        createdAt: finishedAt,
        updatedAt: finishedAt,
      },
      items: [],
    });
  }
  return clone(details);
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
    currentVersion: __APP_VERSION__,
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

function previewScriptVersion(
  definitionId: string,
  versionNumber: number,
  request: SavePersonalScriptVersionRequest,
): PersonalScriptVersion {
  const now = Date.now();
  const bodySha256 = 'a'.repeat(64);
  const warnings = request.body.includes('rm -rf')
    ? [{ code: 'recursive_delete', message: '检测到递归删除操作', lineNumber: 1 }]
    : [];
  return {
    id: crypto.randomUUID(),
    definitionId,
    versionNumber,
    body: request.body,
    bodySha256,
    parameters: clone(request.parameters),
    scanSummary: {
      lineCount: request.body.split('\n').length,
      characterCount: [...request.body].length,
      bodySha256,
      warningCount: warnings.length,
      warnings,
    },
    timeoutSeconds: request.timeoutSeconds,
    shell: request.shell,
    compatibility: request.shell === 'powershell'
      ? { osFamilies: ['windows', 'linux', 'macos'], requiredCommands: ['powershell_or_pwsh'] }
      : { osFamilies: ['linux', 'bsd'], requiredCommands: [request.shell === 'bash' ? 'bash' : 'sh'] },
    createdAt: now,
  };
}

function previewScriptSummary(details: PersonalScriptDetails): PersonalScriptSummary {
  return {
    id: details.definition.id,
    title: details.definition.title,
    category: details.definition.category,
    tags: clone(details.definition.tags),
    isFavorite: details.definition.isFavorite,
    isEnabled: details.definition.isEnabled,
    activeVersionId: details.activeVersion.id,
    activeVersionNumber: details.activeVersion.versionNumber,
    bodySha256: details.activeVersion.bodySha256,
    shell: details.activeVersion.shell,
    compatibility: clone(details.activeVersion.compatibility),
    updatedAt: details.definition.updatedAt,
  };
}

function requirePreviewScript(scriptId: string): PersonalScriptDetails {
  const details = previewPersonalScripts.get(scriptId);
  if (!details) {
    throw Object.assign(new Error('脚本不存在或已删除。'), { code: 'validation' });
  }
  return details;
}

function createPreviewPersonalScript(
  request: CreatePersonalScriptRequest,
): PersonalScriptDetails {
  const id = crypto.randomUUID();
  const now = Date.now();
  const version = previewScriptVersion(id, 1, request);
  const details: PersonalScriptDetails = {
    definition: {
      id,
      title: request.title,
      category: request.category,
      tags: clone(request.tags),
      isFavorite: false,
      isEnabled: false,
      activeVersionId: version.id,
      createdAt: now,
      updatedAt: now,
      deletedAt: null,
    },
    activeVersion: version,
  };
  previewPersonalScripts.set(id, details);
  previewPersonalScriptVersions.set(id, [version]);
  return clone(details);
}

function previewMigration(target: string, retryable: boolean): DataMigrationPreview {
  return {
    previewId: crypto.randomUUID(),
    confirmationToken: crypto.randomUUID(),
    expiresAt: Date.now() + 5 * 60 * 1000,
    source: previewDataRoot,
    target,
    fileCount: 128,
    totalBytes: 42 * 1024 * 1024,
    requiredBytes: 106 * 1024 * 1024,
    availableBytes: 300 * 1024 * 1024 * 1024,
    oldRootWillBeKept: true,
    retryable,
  };
}

export const previewApi = {
  ...updatePreviewApi,
  ...workflowPreviewApi,
  bootstrapStatus: async () => ({
    state: 'ready' as const,
    dataRoot: previewDataRoot,
    dataRootSource: 'platform' as const,
    dataRootMutable: true,
    lastDataMigration: previewDataMigration,
  }),
  initializeDataRoot: async (path: string) => ({
    state: 'ready' as const,
    dataRoot: path,
    dataRootSource: 'platform' as const,
    dataRootMutable: true,
    lastDataMigration: null,
  }),
  preflightDataRootMigration: async (targetPath: string): Promise<DataMigrationPreview> => {
    previewDataMigrationTarget = targetPath;
    return previewMigration(targetPath, false);
  },
  preflightRetryDataRootMigration: async (): Promise<DataMigrationPreview> => {
    const target = previewDataMigration?.target ?? `${previewDataRoot}-migrated`;
    previewDataMigrationTarget = target;
    return previewMigration(target, true);
  },
  preflightPortableDefaultDataRootMigration: async (): Promise<DataMigrationPreview> => {
    const target = `${previewDataRoot}\\portable-data`;
    previewDataMigrationTarget = target;
    return previewMigration(target, false);
  },
  startDataRootMigration: async (
    previewId: string,
    _confirmationToken: string,
  ): Promise<DataMigrationJournal> => {
    const now = Date.now();
    previewDataMigration = {
      schemaVersion: 1,
      migrationId: previewId,
      source: previewDataRoot,
      target: previewDataMigrationTarget ?? `${previewDataRoot}-migrated`,
      sourceMode: 'platform',
      parentPid: 1,
      fileCount: 128,
      totalBytes: 42 * 1024 * 1024,
      copiedFiles: 0,
      copiedBytes: 0,
      phase: 'prepared',
      errorSummary: null,
      startedAt: now,
      updatedAt: now,
      acknowledged: false,
    };
    return previewDataMigration;
  },
  getDataRootMigrationStatus: async () => previewDataMigration,
  acknowledgeDataRootMigration: async (_migrationId: string) => {
    if (!previewDataMigration) throw new Error('没有迁移结果');
    previewDataMigration = { ...previewDataMigration, acknowledged: true };
    return previewDataMigration;
  },
  openDataRootFolder: async (_kind: 'current' | 'last_source') => undefined,
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
    probeSchemaVersion: 1,
    platformFamily: 'linux',
    remoteShell: 'bash',
    pathStyle: 'posix',
    osId: 'openEuler',
    osFamily: 'RHEL / 国产 Linux',
    versionId: '24.03 LTS',
    packageManager: 'dnf',
    serviceManager: 'systemd',
    architecture: 'x86_64',
    shell: '/bin/bash',
    commands: ['grep', 'gzip', 'awk', 'systemctl', 'ps', 'df', 'sh'],
    services: ['nginx.service', 'sshd.service'],
    containers: ['web'],
    interfaces: [{
      name: 'eth0',
      isUp: true,
      isDefault: true,
      addresses: ['192.0.2.10/24'],
      gateway4: '192.0.2.1',
      gateway6: null,
    }],
    dnsServers: ['223.5.5.5'],
    currentTimezone: 'Asia/Shanghai',
    currentTime: '2026-08-07T00:20:00+08:00',
    ntpEnabled: true,
    ntpSynchronized: true,
    timezones: ['Asia/Shanghai', 'Asia/Hong_Kong', 'UTC'],
  }),
  listPersonalScripts: async (
    filter: PersonalScriptListFilter,
  ): Promise<PersonalScriptSummary[]> =>
    [...previewPersonalScripts.values()]
      .filter((details) =>
        !filter.query ||
        details.definition.title.includes(filter.query) ||
        details.definition.category.includes(filter.query),
      )
      .filter((details) => !filter.category || details.definition.category === filter.category)
      .filter((details) => !filter.tag || details.definition.tags.includes(filter.tag))
      .filter((details) => filter.favorite === undefined || details.definition.isFavorite === filter.favorite)
      .filter((details) => filter.enabled === undefined || details.definition.isEnabled === filter.enabled)
      .map(previewScriptSummary),
  getPersonalScriptForEditor: async (scriptId: string) => {
    const details = previewPersonalScripts.get(scriptId);
    return details ? clone(details) : null;
  },
  listPersonalScriptVersions: async (scriptId: string) =>
    clone(previewPersonalScriptVersions.get(scriptId) ?? []),
  createPersonalScript: async (
    request: CreatePersonalScriptRequest,
  ): Promise<PersonalScriptDetails> => createPreviewPersonalScript(request),
  savePersonalScriptVersion: async (
    scriptId: string,
    request: SavePersonalScriptVersionRequest,
  ): Promise<PersonalScriptVersion> => {
    const details = requirePreviewScript(scriptId);
    const versions = previewPersonalScriptVersions.get(scriptId) ?? [];
    const version = previewScriptVersion(scriptId, versions.length + 1, request);
    details.activeVersion = version;
    details.definition.activeVersionId = version.id;
    details.definition.updatedAt = Date.now();
    previewPersonalScriptVersions.set(scriptId, [version, ...versions]);
    return clone(version);
  },
  updatePersonalScriptMetadata: async (
    scriptId: string,
    request: UpdatePersonalScriptMetadataRequest,
  ) => {
    const details = requirePreviewScript(scriptId);
    details.definition.title = request.title;
    details.definition.category = request.category;
    details.definition.tags = clone(request.tags);
    details.definition.updatedAt = Date.now();
  },
  copyPersonalScript: async (scriptId: string): Promise<PersonalScriptDetails> => {
    const source = requirePreviewScript(scriptId);
    return createPreviewPersonalScript({
      title: `${source.definition.title} 副本`.slice(0, 80),
      category: source.definition.category,
      tags: clone(source.definition.tags),
      body: source.activeVersion.body,
      parameters: clone(source.activeVersion.parameters),
      timeoutSeconds: source.activeVersion.timeoutSeconds,
      shell: source.activeVersion.shell,
    });
  },
  setPersonalScriptFavorite: async (scriptId: string, favorite: boolean) => {
    const details = requirePreviewScript(scriptId);
    details.definition.isFavorite = favorite;
    details.definition.updatedAt = Date.now();
  },
  setPersonalScriptEnabled: async (scriptId: string, enabled: boolean) => {
    const details = requirePreviewScript(scriptId);
    details.definition.isEnabled = enabled;
    details.definition.updatedAt = Date.now();
  },
  deletePersonalScript: async (scriptId: string) => {
    requirePreviewScript(scriptId);
    previewPersonalScripts.delete(scriptId);
  },
  importPersonalScript: async (packageJson: string): Promise<PersonalScriptDetails> => {
    const packageValue = JSON.parse(packageJson) as {
      schemaVersion?: number;
      script?: CreatePersonalScriptRequest;
    };
    if (packageValue.schemaVersion !== 1 || !packageValue.script) {
      throw Object.assign(new Error('脚本包版本或结构不受支持。'), {
        code: 'unsupported_script_package',
      });
    }
    return createPreviewPersonalScript({
      ...packageValue.script,
      timeoutSeconds: 300,
    });
  },
  exportPersonalScript: async (scriptId: string): Promise<ScriptPackageExport> => {
    requirePreviewScript(scriptId);
    return {
      relativePath: `downloads/scripts/script-${crypto.randomUUID()}.json`,
      sha256: 'a'.repeat(64),
      sizeBytes: 1024,
    };
  },
  previewPersonalScriptRun: async (
    scriptId: string,
    serverId: string,
    _parameterValues: Record<string, unknown>,
  ): Promise<PersonalScriptRunPreview> => {
    const details = requirePreviewScript(scriptId);
    if (!details.definition.isEnabled) {
      throw Object.assign(new Error('脚本尚未启用。'), { code: 'validation' });
    }
    const preview: PersonalScriptRunPreview = {
      previewId: crypto.randomUUID(),
      confirmationToken: crypto.randomUUID(),
      expiresAt: Date.now() + 5 * 60 * 1000,
      serverId,
      scriptDefinitionId: scriptId,
      scriptVersionId: details.activeVersion.id,
      scriptVersionNumber: details.activeVersion.versionNumber,
      title: details.definition.title,
      riskLevel: 'dangerous',
      automaticRollbackAvailable: false,
      warning: '不可自动回滚：请确认目标服务器和参数后再运行。',
      lineCount: details.activeVersion.scanSummary.lineCount,
      characterCount: details.activeVersion.scanSummary.characterCount,
      bodySha256: details.activeVersion.bodySha256,
      parameterNames: details.activeVersion.parameters.map((parameter) => parameter.name),
      scanWarnings: clone(details.activeVersion.scanSummary.warnings),
      timeoutSeconds: details.activeVersion.timeoutSeconds,
      shell: details.activeVersion.shell,
      compatibility: clone(details.activeVersion.compatibility),
    };
    previewPersonalScriptRuns.set(preview.previewId, preview);
    return clone(preview);
  },
  confirmPersonalScriptRun: async (
    request: ConfirmPersonalScriptRunRequest,
    onEvent: (event: ExecutionEvent) => void,
  ): Promise<PersonalScriptRunResult> => {
    const preview = previewPersonalScriptRuns.get(request.previewId);
    if (!preview || preview.confirmationToken !== request.confirmationToken) {
      throw Object.assign(new Error('脚本运行确认无效。'), {
        code: 'script_confirmation_required',
      });
    }
    previewPersonalScriptRuns.delete(request.previewId);
    const execution = createPreviewExecution(preview.serverId, 'script.personal');
    emitPreview(onEvent, execution);
    return {
      operationRunId: preview.previewId,
      scriptDefinitionId: preview.scriptDefinitionId,
      scriptVersionId: preview.scriptVersionId,
      execution,
    };
  },
  cancelPersonalScriptRun: async (operationRunId: string) => {
    previewPersonalScriptRuns.delete(operationRunId);
  },
  listTaskDefinitions: async (_serverId: string) => previewTasks,
  getTaskLibrarySnapshot: async (_serverId: string, _forceRefresh = false) => ({
    tasks: previewTasks,
    capabilities: await previewApi.testConnection(_serverId),
    detectedAt: Date.now(),
    cacheExpiresAt: Date.now() + 300_000,
  }),
  previewTaskRemediation: async (
    _serverId: string,
    taskId: string,
  ): Promise<TaskRemediationPreview> => ({
    previewId: crypto.randomUUID(),
    confirmationToken: crypto.randomUUID(),
    expiresAt: Date.now() + 5 * 60 * 1000,
    taskId,
    implementationId: 'preview-remediation',
    missingCommands: ['tcpdump'],
    packages: ['tcpdump'],
    packageManager: 'apt',
    permissionState: 'ready',
    commandSummary: 'apt-get install -y --no-install-recommends tcpdump',
  }),
  confirmTaskRemediation: async (
    serverId: string,
    _request: ConfirmTaskRemediationRequest,
    onEvent: (event: ExecutionEvent) => void,
  ): Promise<TaskAvailability> => {
    const execution = createPreviewExecution(serverId, 'maintenance.package_install');
    emitPreview(onEvent, execution);
    const task = previewTasks.find((item) => item.definition.id === 'network.packet_capture');
    return task ?? previewTasks[0]!;
  },
  listOperationsTasks: async (_serverId: string) => previewTasks,
  preflightOperation: async (serverId: string, request: OperationPreflightRequest) =>
    createOperationPreview(serverId, request),
  previewOperation: async (serverId: string, request: OperationPreflightRequest) =>
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
    return executePreviewOperation(serverId, request, previewId, onEvent);
  },
  confirmOperation: async (
    serverId: string,
    request: OperationConfirmRequest,
    onEvent: (event: ExecutionEvent) => void,
  ): Promise<OperationRunDetails> =>
    executePreviewOperation(serverId, request, request.confirmationToken, onEvent),
  listOperationRestorePoints: async (runId: string): Promise<OperationRestoreDetails[]> => {
    const restore = previewOperationRestorePoints.get(runId);
    return restore ? [clone(restore)] : [];
  },
  rollbackOperation: async (restorePointId: string): Promise<OperationRecoveryResult> => {
    const entry = [...previewOperationRestorePoints.entries()]
      .find(([, details]) => details.point.id === restorePointId);
    if (!entry || entry[1].point.status !== 'available') {
      throw Object.assign(new Error('恢复点不存在或已经使用。'), {
        code: 'restore_point_already_consumed',
      });
    }
    const [runId, restore] = entry;
    const operation = requirePreviewOperation(runId);
    const now = Date.now();
    restore.point.status = 'rolled_back';
    restore.point.updatedAt = now;
    operation.run.status = 'rolled_back';
    operation.run.updatedAt = now;
    operation.run.finishedAt = now;
    return clone({
      operation,
      whatHappened: '已按恢复点还原修改前状态',
      serverMayHaveChanged: false,
      stateConfirmed: true,
      nextStep: '请重新检查对应服务或配置是否恢复正常',
      restorePoint: restore.point,
      technicalDetails: null,
    });
  },
  inspectUncertainOperation: async (runId: string): Promise<OperationRecoveryResult> => {
    const operation = requirePreviewOperation(runId);
    if (operation.run.status !== 'uncertain') {
      throw Object.assign(new Error('只有状态未确认的 IP 修改可以重新检查。'), {
        code: 'validation',
      });
    }
    const restore = previewOperationRestorePoints.get(runId);
    return clone({
      operation,
      whatHappened: '服务器仍在自动恢复保护窗口内，当前状态尚未最终确认',
      serverMayHaveChanged: true,
      stateConfirmed: false,
      nextStep: '等待自动恢复窗口结束后再次检查，不要重复修改网络',
      restorePoint: restore?.point ?? null,
      technicalDetails: null,
    });
  },
  cleanupOperationRestoreAssets: async (
    restorePointId: string,
  ): Promise<OperationRestoreDetails> => {
    const entry = [...previewOperationRestorePoints.entries()]
      .find(([, details]) => details.point.id === restorePointId);
    if (!entry) {
      throw Object.assign(new Error('恢复点不存在。'), { code: 'validation' });
    }
    const restore = entry[1];
    const expired = restore.point.expiresAt !== null && restore.point.expiresAt <= Date.now();
    if (restore.point.status !== 'rolled_back' && !expired) {
      throw Object.assign(new Error('只能清理已经使用或已经过期的恢复资产。'), {
        code: 'validation',
      });
    }
    restore.point.status = 'expired';
    restore.point.remoteAssetId = null;
    restore.point.updatedAt = Date.now();
    return clone(restore);
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
  startOperationBatch: async (
    request: OperationBatchRequest,
  ): Promise<OperationBatchDetails> => {
    const id = crypto.randomUUID();
    const now = Date.now();
    const details: OperationBatchDetails = {
      batch: {
        id,
        taskId: request.taskId,
        taskVersion: request.taskVersion,
        status: 'succeeded',
        createdAt: now,
        finishedAt: now,
      },
      items: request.serverIds.map((serverId) => ({
        batchId: id,
        serverId,
        operationRunId: null,
        status: 'succeeded',
        errorMessage: null,
      })),
    };
    previewOperationBatches.set(id, details);
    return clone(details);
  },
  cancelOperationBatch: async (batchId: string) => {
    const details = previewOperationBatches.get(batchId);
    if (!details) throw Object.assign(new Error('批量任务不存在。'), { code: 'validation' });
    details.batch.status = 'cancelled';
    details.batch.finishedAt = Date.now();
    details.items = details.items.map((item) =>
      item.status === 'queued' || item.status === 'running'
        ? { ...item, status: 'cancelled' }
        : item,
    );
  },
  getOperationBatch: async (batchId: string) => {
    const details = previewOperationBatches.get(batchId);
    return details ? clone(details) : null;
  },
  exportOperationReport: async (runId: string, format: ReportFormat): Promise<ExecutionFile> => ({
    id: crypto.randomUUID(),
    relativePath: `downloads/reports/operation-${runId}.${format}`,
    purpose: 'operation_report',
    sizeBytes: 0,
    sha256: 'preview-report-sha256',
  }),
  exportOperationBatchReport: async (
    batchId: string,
    format: ReportFormat,
  ): Promise<ExecutionFile> => ({
    id: crypto.randomUUID(),
    relativePath: `downloads/reports/batch-${batchId}.${format}`,
    purpose: 'operation_batch_report',
    sizeBytes: 0,
    sha256: 'preview-batch-report-sha256',
  }),
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
  createRemoteDirectory: async (_serverId: string, _parent: string, _name: string) => undefined,
  renameRemoteEntry: async (_serverId: string, _path: string, _newName: string) => undefined,
  deleteRemoteEntry: async (_serverId: string, _path: string, _expectedKind: 'directory' | 'file' | 'symlink' | 'other') => undefined,
  searchLogs: async (
    serverId: string,
    request: LogSearchRequest,
    onEvent: (event: ExecutionEvent) => void,
  ) => {
    previewLogTarget = request.target;
    previewLogKeyword = request.keyword;
    previewLogCaseSensitive = request.caseSensitive;
    const details = createPreviewExecution(serverId, 'logs.search');
    emitPreview(onEvent, details);
    return details;
  },
  readLogResultPage: async (_executionId: string, cursor: string | null, pageSize: number) => {
    if (previewLogTarget === 'filename') {
      const files = [
        {
          resultType: 'file' as const,
          path: '/home/app/requirements.txt',
          name: 'requirements.txt',
          size: 96,
          modifiedAt: 1_785_801_600,
        },
      ];
      const keyword = previewLogCaseSensitive ? previewLogKeyword : previewLogKeyword.toLocaleLowerCase();
      return {
        items: files.filter((file) => {
          const candidate = `${file.name} ${file.path}`;
          return (previewLogCaseSensitive ? candidate : candidate.toLocaleLowerCase()).includes(keyword);
        }),
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
    request: UploadRequest,
    onEvent: (event: ExecutionEvent) => void,
  ) => {
    const details = createPreviewExecution(serverId, 'transfer.upload');
    emitTransferPreview(onEvent, details, request.remotePath);
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
    emitTransferPreview(onEvent, details, `downloads/${request.suggestedName}`);
    return details;
  },
  enqueueUploadFile: async (serverId: string, request: UploadRequest) =>
    createPreviewTransferJob(
      serverId,
      'upload',
      request.localPath,
      request.remotePath,
      request.overwrite,
      request.verification,
    ),
  enqueueDownloadFile: async (serverId: string, request: DownloadRequest) =>
    createPreviewTransferJob(
      serverId,
      'download',
      request.remotePath,
      request.suggestedName,
      request.overwrite,
      request.verification,
    ),
  listTransferJobs: async (serverId: string | null) =>
    previewTransferJobs.filter((job) => !serverId || job.serverId === serverId),
  cancelTransferJob: async (jobId: string) => {
    const job = previewTransferJobs.find((item) => item.id === jobId);
    if (!job) throw new Error('传输任务不存在');
    job.status = 'cancelled';
    job.cancelRequested = true;
    job.finishedAt = Date.now();
    return job;
  },
  retryTransferJob: async (jobId: string) => {
    const job = previewTransferJobs.find((item) => item.id === jobId);
    if (!job) throw new Error('传输任务不存在');
    job.status = 'queued';
    job.cancelRequested = false;
    job.errorCategory = null;
    job.errorMessage = null;
    job.finishedAt = null;
    return job;
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
