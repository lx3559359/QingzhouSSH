import type {
  CreateServerRequest,
  CustomExecutionRequest,
  DownloadRequest,
  ExecutionDetails,
  ExecutionEvent,
  ExecutionFilter,
  LogSearchRequest,
  HostKeyCheck,
  HostKeyObservation,
  ServerProfile,
  SystemCapabilities,
  TaskAvailability,
  TaskExecutionRequest,
  UploadRequest,
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

const previewTasks: TaskAvailability[] = [
  {
    compatible: true,
    reason: null,
    definition: {
      id: 'system.overview',
      version: 1,
      category: 'system',
      title: '系统概览',
      description: '查看运行时间、负载、内存和磁盘摘要',
      riskLevel: 'safe',
      parameters: [],
      implementations: [
        {
          id: 'posix',
          compatibility: {
            osFamilies: ['debian', 'rhel', 'openeuler'],
            serviceManagers: [],
            requiredCommands: ['uptime'],
          },
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
      version: 1,
      category: 'service',
      title: '重启服务',
      description: '重启指定 systemd 服务',
      riskLevel: 'dangerous',
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

export const previewApi = {
  bootstrapStatus: async () => ({
    state: 'ready' as const,
    dataRoot: 'D:\\QingzhouSSH\\data',
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
  searchLogs: async (
    serverId: string,
    _request: LogSearchRequest,
    onEvent: (event: ExecutionEvent) => void,
  ) => {
    const details = createPreviewExecution(serverId, 'logs.search');
    emitPreview(onEvent, details);
    return details;
  },
  readLogResultPage: async (_executionId: string, cursor: string | null, pageSize: number) => {
    const start = cursor ? Number(cursor) : 0;
    const items = Array.from({ length: Math.min(pageSize, 8) }, (_, index) => ({
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
