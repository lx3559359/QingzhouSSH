import '@testing-library/jest-dom/vitest';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({ startCustomExecution: vi.fn(), cancelExecution: vi.fn() }));
vi.mock('../../api/tauri', () => ({ api: apiMocks }));

import type { ExecutionDetails, ServerProfile } from '../../api/contracts';
import { AdvancedExecutionPanel } from './AdvancedExecutionPanel';

const servers: ServerProfile[] = [{ id: 'server-1', name: 'Anolis 运维机', host: '10.0.0.6', port: 22, username: 'ops', authKind: 'password', credentialId: 'credential-1' }];
const details: ExecutionDetails = { record: { id: 'advanced-1', serverId: 'server-1', taskId: 'advanced.script', taskVersion: 1, category: 'advanced', status: 'succeeded', createdAt: 1, startedAt: 1, finishedAt: 2, durationMs: 1000, exitCode: 0, errorCategory: null, errorMessage: null, retryable: false, parametersSummary: 'script, 2 lines', outputSummary: 'done', remoteProcessGroup: null }, parameters: [], files: [] };

describe('AdvancedExecutionPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.cancelExecution.mockResolvedValue(undefined);
    apiMocks.startCustomExecution.mockImplementation(async (_serverId, _request, onEvent) => {
      onEvent({ type: 'started', executionId: 'advanced-1', startedAt: 1, sequence: 1, emittedAt: 1 });
      onEvent({ type: 'stdout', text: 'done\n', totalBytes: 5, sequence: 2, emittedAt: 2 });
      return details;
    });
  });

  it('switches command/script mode, confirms only a summary, and runs without persisting content', async () => {
    const user = userEvent.setup();
    const localSpy = vi.spyOn(Storage.prototype, 'setItem');
    render(<AdvancedExecutionPanel servers={servers} serverId="server-1" />);

    expect(screen.getByText(/不提供交互式终端/)).toBeVisible();
    await user.click(screen.getByRole('radio', { name: '多行脚本' }));
    await user.clear(screen.getByLabelText('超时秒数'));
    await user.type(screen.getByLabelText('超时秒数'), '90');
    const secretScript = 'echo super-secret-token\nuname -a';
    await user.type(screen.getByLabelText('脚本内容'), secretScript);
    await user.click(screen.getByRole('button', { name: '检查并运行' }));

    const dialog = screen.getByRole('dialog', { name: '确认高级执行' });
    expect(within(dialog).getByText('Anolis 运维机')).toBeVisible();
    expect(within(dialog).getByText('2 行 · 32 字符')).toBeVisible();
    expect(within(dialog).queryByText(secretScript)).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: '确认并运行' }));

    await waitFor(() => expect(apiMocks.startCustomExecution).toHaveBeenCalledWith('server-1', { mode: 'script', content: secretScript, timeoutSeconds: 90, dangerousConfirmed: true }, expect.any(Function)));
    expect(await screen.findByText(/done/)).toBeVisible();
    expect(localSpy).not.toHaveBeenCalled();
    localSpy.mockRestore();
  });

  it('cancels a running non-interactive command by execution id', async () => {
    const user = userEvent.setup();
    apiMocks.startCustomExecution.mockImplementation((_serverId, _request, onEvent) => {
      onEvent({ type: 'started', executionId: 'advanced-running', startedAt: 1, sequence: 1, emittedAt: 1 });
      return new Promise(() => {});
    });
    render(<AdvancedExecutionPanel servers={servers} serverId="server-1" />);
    await user.type(screen.getByLabelText('命令内容'), 'uptime');
    await user.click(screen.getByRole('button', { name: '检查并运行' }));
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: '确认并运行' }));
    await user.click(await screen.findByRole('button', { name: '取消执行' }));
    expect(apiMocks.cancelExecution).toHaveBeenCalledWith('advanced-running');
  });
});
