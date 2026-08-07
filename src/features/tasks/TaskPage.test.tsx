import '@testing-library/jest-dom/vitest';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  listServers: vi.fn(),
  listTaskDefinitions: vi.fn(),
  getTaskLibrarySnapshot: vi.fn(),
  startTaskExecution: vi.fn(),
  previewOperation: vi.fn(),
  confirmOperation: vi.fn(),
  previewTaskRemediation: vi.fn(),
  confirmTaskRemediation: vi.fn(),
  cancelExecution: vi.fn(),
  listPersonalScripts: vi.fn(),
  getPersonalScriptForEditor: vi.fn(),
  previewPersonalScriptRun: vi.fn(),
  confirmPersonalScriptRun: vi.fn(),
  cancelPersonalScriptRun: vi.fn(),
}));

vi.mock('../../api/tauri', () => ({ api: apiMocks }));

import type {
  ExecutionDetails,
  PersonalScriptDetails,
  PersonalScriptSummary,
  ServerProfile,
  SystemCapabilities,
  TaskAvailability,
} from '../../api/contracts';
import { TaskPage } from './TaskPage';

const server: ServerProfile = {
  id: 'server-1',
  name: 'openEuler 生产机',
  host: '10.0.0.8',
  port: 22,
  username: 'ops',
  authKind: 'password',
  credentialId: 'credential-1',
};

const capabilities: SystemCapabilities = {
  osId: 'openeuler',
  osFamily: 'openeuler',
  versionId: '24.03',
  packageManager: 'dnf',
  serviceManager: 'systemd',
  architecture: 'x86_64',
  shell: '/bin/bash',
  commands: ['ip', 'systemctl', 'timedatectl'],
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
  dnsServers: ['1.1.1.1'],
  currentTimezone: 'Asia/Shanghai',
  currentTime: '2026-08-07T00:20:00+08:00',
  ntpEnabled: true,
  ntpSynchronized: true,
  timezones: ['Asia/Shanghai', 'UTC'],
};

const readyLibrary = {
  source: 'reviewed_command' as const,
  primaryCategory: 'daily_inspection' as const,
  keywords: ['日常巡检'],
  noviceAliases: ['检查服务器'],
};

const tasks: TaskAvailability[] = [
  {
    state: 'ready',
    summary: '当前服务器可以直接运行',
    missingCommands: [],
    remediation: null,
    library: readyLibrary,
    definition: {
      id: 'system.overview',
      version: 1,
      category: 'system',
      title: '系统概览',
      description: '查看系统状态',
      riskLevel: 'safe',
      estimatedSeconds: 30,
      privilege: 'current_user',
      scope: 'read_only_batch',
      parameters: [],
      implementations: [],
      outputKind: 'key_value',
    },
  },
  {
    state: 'ready',
    summary: '当前服务器可以直接运行',
    missingCommands: [],
    remediation: null,
    library: { ...readyLibrary, source: 'builtin_task', primaryCategory: 'service_management' },
    definition: {
      id: 'service.restart',
      version: 1,
      category: 'service',
      title: '重启服务',
      description: '重启指定服务',
      riskLevel: 'dangerous',
      estimatedSeconds: 30,
      privilege: 'root_or_passwordless_sudo',
      scope: 'single_server',
      parameters: [
        {
          name: 'service',
          label: '服务名',
          description: 'systemd 服务名',
          kind: { type: 'serviceName' },
          required: true,
          defaultValue: null,
          sensitive: false,
        },
      ],
      implementations: [],
      outputKind: 'text',
    },
  },
  {
    state: 'ready',
    summary: '当前服务器可以直接运行',
    missingCommands: [],
    remediation: null,
    library: { ...readyLibrary, source: 'builtin_task', primaryCategory: 'service_management' },
    definition: {
      id: 'service.cron_manage',
      version: 2,
      category: 'service',
      title: '管理计划任务',
      description: '只管理本工具创建的计划任务',
      riskLevel: 'dangerous',
      estimatedSeconds: 75,
      privilege: 'root_or_passwordless_sudo',
      scope: 'single_server',
      parameters: [
        {
          name: 'action',
          label: '操作',
          description: '新增、停用或移除',
          kind: { type: 'enum', options: ['add', 'disable', 'remove'] },
          required: true,
          defaultValue: null,
          sensitive: false,
        },
        {
          name: 'schedule',
          label: '执行周期',
          description: '五段 Cron 表达式',
          kind: { type: 'cronExpression' },
          required: false,
          defaultValue: '0 2 * * *',
          sensitive: false,
        },
        {
          name: 'entryId',
          label: '任务标识',
          description: '由客户端自动生成',
          kind: { type: 'managedId' },
          required: true,
          defaultValue: null,
          sensitive: false,
        },
        {
          name: 'task',
          label: '受控任务',
          description: '仅允许内置任务',
          kind: { type: 'enum', options: ['system.overview', 'system.disk_usage'] },
          required: false,
          defaultValue: 'system.overview',
          sensitive: false,
        },
      ],
      implementations: [],
      outputKind: 'text',
    },
  },
];

const unavailableTasks: TaskAvailability[] = [
  {
    ...tasks[0],
    state: 'permission_blocked',
    summary: '需要 root 或免密 sudo',
    definition: {
      ...tasks[0].definition,
      id: 'network.udp',
      title: 'UDP 探测',
      description: '检查 UDP 服务是否可达',
      category: 'network',
    },
    library: {
      ...readyLibrary,
      primaryCategory: 'network',
      noviceAliases: ['UDP 不通'],
    },
  },
  {
    ...tasks[0],
    state: 'unsupported',
    summary: '延迟回滚与重连验证尚未实现',
    definition: {
      ...tasks[0].definition,
      id: 'network.ip_change',
      title: '修改 IP 地址',
      description: '安全能力尚未完成',
      category: 'network',
    },
    library: {
      ...readyLibrary,
      source: 'builtin_task',
      primaryCategory: 'network',
      noviceAliases: ['修改地址'],
    },
  },
];

const remediableTask: TaskAvailability = {
  ...tasks[0],
  state: 'remediable',
  summary: '缺少抓包组件，可以在确认后安全补齐',
  missingCommands: ['tcpdump'],
  remediation: {
    packageManager: 'dnf',
    missingCommands: ['tcpdump'],
    packages: ['tcpdump'],
  },
  definition: {
    ...tasks[0].definition,
    id: 'network.packet_capture',
    title: '限时抓包摘要',
    description: '抓取限定数量的数据包并生成摘要',
    category: 'network',
  },
  library: {
    ...readyLibrary,
    primaryCategory: 'network',
    noviceAliases: ['网络抓包'],
  },
};

const personalScript: PersonalScriptDetails = {
  definition: {
    id: 'script-1',
    title: '服务巡检脚本',
    category: '日常巡检',
    tags: ['服务', '巡检'],
    isFavorite: true,
    isEnabled: true,
    activeVersionId: 'version-1',
    createdAt: 1,
    updatedAt: 1,
    deletedAt: null,
  },
  activeVersion: {
    id: 'version-1',
    definitionId: 'script-1',
    versionNumber: 1,
    body: 'systemctl --failed',
    bodySha256: 'a'.repeat(64),
    parameters: [],
    scanSummary: {
      lineCount: 1,
      characterCount: 18,
      bodySha256: 'a'.repeat(64),
      warningCount: 0,
      warnings: [],
    },
    timeoutSeconds: 30,
    createdAt: 1,
  },
};

const personalScriptSummary: PersonalScriptSummary = {
  id: personalScript.definition.id,
  title: personalScript.definition.title,
  category: personalScript.definition.category,
  tags: personalScript.definition.tags,
  isFavorite: true,
  isEnabled: true,
  activeVersionId: personalScript.activeVersion.id,
  activeVersionNumber: 1,
  bodySha256: personalScript.activeVersion.bodySha256,
  updatedAt: 1,
};

function details(taskId: string): ExecutionDetails {
  return {
    record: {
      id: 'execution-1',
      serverId: server.id,
      taskId,
      taskVersion: 1,
      category: taskId.split('.')[0],
      status: 'succeeded',
      createdAt: 1,
      startedAt: 1,
      finishedAt: 2,
      durationMs: 1,
      exitCode: 0,
      errorCategory: null,
      errorMessage: null,
      retryable: false,
      parametersSummary: null,
      outputSummary: 'ok',
      remoteProcessGroup: null,
    },
    parameters: [],
    files: [],
  };
}

describe('TaskPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.listServers.mockResolvedValue([server]);
    apiMocks.listTaskDefinitions.mockResolvedValue(tasks);
    apiMocks.getTaskLibrarySnapshot.mockResolvedValue({
      tasks,
      capabilities,
      detectedAt: 1,
      cacheExpiresAt: 300_001,
    });
    apiMocks.startTaskExecution.mockImplementation(
      async (_serverId, request, onEvent) => {
        onEvent({
          type: 'stdout',
          sequence: 1,
          emittedAt: 1,
          text: 'load average: 0.12\n',
          totalBytes: 19,
        });
        return details(request.taskId);
      },
    );
    apiMocks.cancelExecution.mockResolvedValue(undefined);
    apiMocks.listPersonalScripts.mockResolvedValue([]);
    apiMocks.getPersonalScriptForEditor.mockResolvedValue(null);
    apiMocks.previewPersonalScriptRun.mockResolvedValue({
      previewId: 'script-preview-1',
      confirmationToken: 'script-token-1',
      expiresAt: Date.now() + 300_000,
      serverId: server.id,
      scriptDefinitionId: 'script-1',
      scriptVersionId: 'version-1',
      scriptVersionNumber: 1,
      title: '服务巡检脚本',
      riskLevel: 'dangerous',
      automaticRollbackAvailable: false,
      warning: '个人脚本无法自动回滚，请确认后运行。',
      lineCount: 1,
      characterCount: 18,
      bodySha256: 'a'.repeat(64),
      parameterNames: [],
      scanWarnings: [],
      timeoutSeconds: 30,
    });
    apiMocks.confirmPersonalScriptRun.mockResolvedValue({
      operationRunId: 'script-preview-1',
      scriptDefinitionId: 'script-1',
      scriptVersionId: 'version-1',
      execution: details('script.personal'),
    });
    apiMocks.cancelPersonalScriptRun.mockResolvedValue(undefined);
    apiMocks.previewOperation.mockResolvedValue({
      previewId: 'preview-1',
      serverId: 'server-1',
      taskId: 'service.restart',
      taskVersion: 1,
      implementationId: 'systemd',
      riskLevel: 'dangerous',
      privilege: 'root_or_passwordless_sudo',
      scope: 'single_server',
      status: 'waiting_confirmation',
      stepTitles: ['预演', '备份', '执行', '验证'],
      estimatedSeconds: 30,
      confirmationToken: 'token-1',
      server: { id: 'server-1', name: server.name, host: server.host, port: 22, username: 'ops' },
      permissionSummary: '使用免密 sudo',
      currentStateSummary: '服务正在运行',
      targetStateSummary: '安全重启服务',
      backupSummary: ['记录当前运行状态'],
      disconnectRisk: { mayDisconnect: false, explanation: null, automaticRecoverySeconds: null },
    });
    apiMocks.confirmOperation.mockResolvedValue({ run: { id: 'run-1' }, steps: [] });
    apiMocks.previewTaskRemediation.mockResolvedValue({
      previewId: 'remediation-preview-1',
      confirmationToken: 'remediation-token-1',
      expiresAt: Date.now() + 300_000,
      taskId: 'network.packet_capture',
      implementationId: 'linux-tcpdump',
      missingCommands: ['tcpdump'],
      packages: ['tcpdump'],
      packageManager: 'dnf',
      permissionState: 'ready',
      commandSummary: '将通过 dnf 安装白名单组件：tcpdump',
    });
    apiMocks.confirmTaskRemediation.mockResolvedValue({ ...remediableTask, state: 'ready' });
  });

  it('keeps category, list and details visible while hiding unavailable tools by default', async () => {
    const user = userEvent.setup();
    apiMocks.getTaskLibrarySnapshot.mockResolvedValue({
      tasks: [...tasks, ...unavailableTasks],
      capabilities,
      detectedAt: 1,
      cacheExpiresAt: 300_001,
    });
    render(<TaskPage />);

    expect(await screen.findByLabelText('工具分类')).toBeVisible();
    expect(screen.getByLabelText('工具列表')).toBeVisible();
    expect(screen.getByLabelText('工具详情')).toBeVisible();
    expect(screen.queryByText('UDP 探测')).not.toBeInTheDocument();
    expect(screen.queryByText('修改 IP 地址')).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: '查看受限与不支持' }));
    expect(await screen.findByText('UDP 探测')).toBeVisible();
    expect(screen.getByText('修改 IP 地址')).toBeVisible();
  });

  it('opens the personal script center as a first-class task mode', async () => {
    const user = userEvent.setup();
    render(<TaskPage />);
    await screen.findByRole('heading', { name: '系统概览' });

    await user.click(screen.getByRole('button', { name: '我的脚本' }));

    expect(await screen.findByRole('region', { name: '个人脚本中心' })).toBeVisible();
    expect(screen.getByRole('heading', { name: '个人脚本' })).toBeVisible();
    expect(apiMocks.listPersonalScripts).toHaveBeenCalledWith({
      query: undefined,
      favorite: undefined,
    });
  });

  it('selects a server, loads compatible cards, and streams task output', async () => {
    const user = userEvent.setup();
    render(<TaskPage />);

    expect(await screen.findByRole('heading', { name: '系统概览' })).toBeVisible();
    expect(apiMocks.getTaskLibrarySnapshot).toHaveBeenCalledWith('server-1', false);
    await user.click(screen.getByRole('button', { name: '选择任务 系统概览' }));
    await user.click(screen.getByRole('button', { name: '运行任务' }));

    expect(await screen.findByText(/load average: 0.12/)).toBeVisible();
    expect(apiMocks.startTaskExecution).toHaveBeenCalledWith(
      'server-1',
      { taskId: 'system.overview', parameters: {}, dangerousConfirmed: false },
      expect.any(Function),
    );
  });

  it('requires a target-and-impact confirmation for dangerous service actions', async () => {
    const user = userEvent.setup();
    render(<TaskPage />);
    await screen.findByRole('heading', { name: '重启服务' });

    await user.click(screen.getByRole('button', { name: '选择任务 重启服务' }));
    await user.selectOptions(screen.getByLabelText('服务名'), 'nginx.service');
    await user.click(screen.getByRole('button', { name: '运行任务' }));

    expect(await screen.findByRole('heading', { name: '确认危险操作' })).toBeVisible();
    const dialog = screen.getByRole('dialog', { name: '确认危险操作' });
    expect(within(dialog).getByText('openEuler 生产机')).toBeVisible();
    expect(within(dialog).getByText(/重启指定服务/)).toBeVisible();
    await user.click(screen.getByRole('button', { name: '确认并运行' }));

    expect(apiMocks.previewOperation).toHaveBeenCalledWith(
      'server-1',
      { taskId: 'service.restart', taskVersion: 1, parameters: { service: 'nginx.service' } },
    );
    await waitFor(() =>
      expect(apiMocks.confirmOperation).toHaveBeenCalledWith(
        'server-1',
        {
          taskId: 'service.restart',
          taskVersion: 1,
          parameters: { service: 'nginx.service' },
          confirmationToken: 'token-1',
        },
        expect.any(Function),
      ),
    );
  });

  it('lets the user refresh detected interfaces and service choices on demand', async () => {
    const user = userEvent.setup();
    render(<TaskPage />);
    await screen.findByRole('heading', { name: '重启服务' });

    await user.click(screen.getByRole('button', { name: '选择任务 重启服务' }));
    await user.click(screen.getByRole('button', { name: '重新检测服务器参数' }));

    await waitFor(() => expect(apiMocks.getTaskLibrarySnapshot).toHaveBeenLastCalledWith('server-1', true));
  });

  it('turns structured backend failures into actionable Chinese guidance', async () => {
    const user = userEvent.setup();
    apiMocks.startTaskExecution.mockRejectedValue({
      code: 'ssh',
      message: 'SSH 操作失败：Connection refused',
      retryable: true,
    });
    render(<TaskPage />);

    await screen.findByRole('heading', { name: '系统概览' });
    await user.click(screen.getByRole('button', { name: '选择任务 系统概览' }));
    await user.click(screen.getByRole('button', { name: '运行任务' }));

    expect(await screen.findByText('无法连接到目标服务器，请确认服务器在线、SSH 地址和端口正确后重试。')).toBeVisible();
    expect(screen.queryByText('[object Object]')).not.toBeInTheDocument();
    await user.click(screen.getByText('查看技术详情'));
    expect(screen.getByText('SSH 操作失败：Connection refused')).toBeVisible();
  });

  it('generates a read-only ownership id for managed cron entries', async () => {
    const user = userEvent.setup();
    render(<TaskPage />);
    await screen.findByRole('heading', { name: '管理计划任务' });

    await user.click(screen.getByRole('button', { name: '选择任务 管理计划任务' }));
    await user.selectOptions(screen.getByLabelText('操作'), 'add');
    const entryId = screen.getByLabelText('任务标识');
    expect(entryId).toHaveAttribute('readonly');
    expect((entryId as HTMLInputElement).value).toMatch(/^[0-9a-f-]{36}$/);
  });

  it('previews missing packages and refreshes compatibility without auto-running the task', async () => {
    const user = userEvent.setup();
    apiMocks.getTaskLibrarySnapshot
      .mockResolvedValueOnce({
        tasks: [...tasks, remediableTask],
        capabilities,
        detectedAt: 1,
        cacheExpiresAt: 300_001,
      })
      .mockResolvedValueOnce({
        tasks: [...tasks, { ...remediableTask, state: 'ready', missingCommands: [], remediation: null }],
        capabilities,
        detectedAt: 2,
        cacheExpiresAt: 300_002,
      });
    render(<TaskPage />);

    await user.click(await screen.findByRole('button', { name: '选择任务 限时抓包摘要' }));
    await user.click(screen.getByRole('button', { name: '查看并补齐组件' }));

    const dialog = await screen.findByRole('dialog', { name: '确认补齐组件' });
    expect(within(dialog).getByText('openEuler 生产机')).toBeVisible();
    expect(within(dialog).getAllByText('tcpdump').length).toBeGreaterThan(0);
    expect(within(dialog).getByText(/不会询问 sudo 密码/)).toBeVisible();
    expect(within(dialog).getByText(/不会自动运行原任务/)).toBeVisible();
    expect(apiMocks.previewTaskRemediation).toHaveBeenCalledWith('server-1', 'network.packet_capture');
    expect(apiMocks.confirmTaskRemediation).not.toHaveBeenCalled();

    await user.click(within(dialog).getByRole('button', { name: '确认安装组件' }));

    await waitFor(() => expect(apiMocks.confirmTaskRemediation).toHaveBeenCalledWith(
      'server-1',
      { previewId: 'remediation-preview-1', confirmationToken: 'remediation-token-1' },
      expect.any(Function),
    ));
    await waitFor(() => expect(apiMocks.getTaskLibrarySnapshot).toHaveBeenCalledTimes(2));
    expect(apiMocks.startTaskExecution).not.toHaveBeenCalledWith(
      'server-1',
      expect.objectContaining({ taskId: 'network.packet_capture' }),
      expect.any(Function),
    );
  });

  it('runs a personal script from the unified library through the existing safety preview', async () => {
    const user = userEvent.setup();
    apiMocks.listPersonalScripts.mockResolvedValue([personalScriptSummary]);
    apiMocks.getPersonalScriptForEditor.mockResolvedValue(personalScript);
    render(<TaskPage />);

    await user.click(await screen.findByRole('button', { name: '选择任务 服务巡检脚本' }));
    await user.click(screen.getByRole('button', { name: '检查并运行脚本' }));

    const dialog = await screen.findByRole('dialog', { name: '运行个人脚本' });
    expect(within(dialog).getByText(/个人脚本无法自动回滚/)).toBeVisible();
    expect(apiMocks.previewPersonalScriptRun).toHaveBeenCalledWith('script-1', 'server-1', {});

    await user.click(within(dialog).getByRole('button', { name: '确认并运行' }));
    await waitFor(() => expect(apiMocks.confirmPersonalScriptRun).toHaveBeenCalledWith(
      { previewId: 'script-preview-1', confirmationToken: 'script-token-1' },
      expect.any(Function),
    ));
  });
});
