import '@testing-library/jest-dom/vitest';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  listServers: vi.fn(),
  listTaskDefinitions: vi.fn(),
  startTaskExecution: vi.fn(),
  cancelExecution: vi.fn(),
  listPersonalScripts: vi.fn(),
}));

vi.mock('../../api/tauri', () => ({ api: apiMocks }));

import type {
  ExecutionDetails,
  ServerProfile,
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

const tasks: TaskAvailability[] = [
  {
    compatible: true,
    reason: null,
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
    compatible: true,
    reason: null,
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
    compatible: true,
    reason: null,
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
    expect(apiMocks.listTaskDefinitions).toHaveBeenCalledWith('server-1');
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
    await user.type(screen.getByLabelText('服务名'), 'nginx.service');
    await user.click(screen.getByRole('button', { name: '运行任务' }));

    expect(screen.getByRole('heading', { name: '确认危险操作' })).toBeVisible();
    const dialog = screen.getByRole('dialog', { name: '确认危险操作' });
    expect(within(dialog).getByText('openEuler 生产机')).toBeVisible();
    expect(within(dialog).getByText(/重启指定服务/)).toBeVisible();
    await user.click(screen.getByRole('button', { name: '确认并运行' }));

    await waitFor(() =>
      expect(apiMocks.startTaskExecution).toHaveBeenCalledWith(
        'server-1',
        {
          taskId: 'service.restart',
          parameters: { service: 'nginx.service' },
          dangerousConfirmed: true,
        },
        expect.any(Function),
      ),
    );
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
});
