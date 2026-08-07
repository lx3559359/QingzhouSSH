import '@testing-library/jest-dom/vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({ listServers: vi.fn(), listExecutions: vi.fn(), getExecution: vi.fn() }));
vi.mock('../../api/tauri', () => ({ api: apiMocks }));

import { ExecutionHistoryPage } from './ExecutionHistoryPage';

const record = { id: 'execution-1', serverId: 'server-1', taskId: 'service.restart', taskVersion: 1, category: 'service', status: 'succeeded' as const, createdAt: Date.parse('2026-08-03T10:00:00Z'), startedAt: Date.parse('2026-08-03T10:00:01Z'), finishedAt: Date.parse('2026-08-03T10:00:02Z'), durationMs: 1000, exitCode: 0, errorCategory: null, errorMessage: null, retryable: false, parametersSummary: 'service=nginx.service', outputSummary: 'nginx restarted; token=[REDACTED]', remoteProcessGroup: null };

describe('ExecutionHistoryPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.listServers.mockResolvedValue([{ id: 'server-1', name: 'openEuler 生产机', host: '10.0.0.8', port: 22, username: 'ops', authKind: 'password', credentialId: 'credential-1' }]);
    apiMocks.listExecutions.mockResolvedValue([record]);
    apiMocks.getExecution.mockResolvedValue({ record, parameters: [{ name: 'service', displayValue: 'nginx.service', sensitive: false }], files: [{ id: 'file-1', relativePath: 'logs/executions/execution-1.log', purpose: 'execution_log', sizeBytes: 128, sha256: 'd'.repeat(64) }] });
  });

  it('filters by server, category, status and time, then shows a redacted execution detail timeline', async () => {
    const user = userEvent.setup();
    render(<ExecutionHistoryPage />);
    expect(await screen.findByText('service.restart')).toBeVisible();
    expect(screen.getByText('中断时仍在运行的记录会被标记为“状态待确认”，避免误报成功。')).toBeVisible();
    expect(screen.queryByText(/running|uncertain/)).not.toBeInTheDocument();
    expect(screen.getByRole('option', { name: '成功' })).toHaveValue('succeeded');
    expect(screen.getAllByText('成功')).toHaveLength(2);
    expect(screen.queryByText('succeeded')).not.toBeInTheDocument();

    await user.selectOptions(screen.getByLabelText('历史服务器'), 'server-1');
    await user.selectOptions(screen.getByLabelText('执行类别'), 'service');
    await user.selectOptions(screen.getByLabelText('执行状态'), 'succeeded');
    fireEvent.change(screen.getByLabelText('开始日期'), { target: { value: '2026-08-03' } });
    await waitFor(() => expect(apiMocks.listExecutions).toHaveBeenLastCalledWith(expect.objectContaining({ serverId: 'server-1', category: 'service', status: 'succeeded', createdFrom: expect.any(Number) })));

    await user.click(screen.getByRole('button', { name: /查看 service.restart/ }));
    expect(await screen.findByRole('heading', { name: '执行详情' })).toBeVisible();
    expect(screen.getAllByText('成功')).toHaveLength(3);
    expect(screen.queryByText('succeeded')).not.toBeInTheDocument();
    expect(screen.getByText('nginx.service')).toBeVisible();
    expect(screen.getByText('退出码 0')).toBeVisible();
    expect(screen.getByText(/token=\[REDACTED\]/)).toBeVisible();
    expect(screen.getByText('logs/executions/execution-1.log')).toBeVisible();
    expect(screen.getByText('已创建')).toBeVisible();
    expect(screen.getByText('已完成')).toBeVisible();
  });
});
