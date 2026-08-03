import '@testing-library/jest-dom/vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  listServers: vi.fn(),
  searchLogs: vi.fn(),
  readLogResultPage: vi.fn(),
  downloadLogResult: vi.fn(),
}));

vi.mock('../../api/tauri', () => ({ api: apiMocks }));

import type { ExecutionDetails } from '../../api/contracts';
import { LogSearchPage } from './LogSearchPage';

const server = {
  id: 'server-1',
  name: '麒麟日志机',
  host: '10.0.0.9',
  port: 22,
  username: 'ops',
  authKind: 'password' as const,
  credentialId: 'credential-1',
};

function details(status: 'succeeded' | 'failed' = 'succeeded'): ExecutionDetails {
  return {
    record: {
      id: 'execution-1',
      serverId: server.id,
      taskId: 'logs.search',
      taskVersion: 1,
      category: 'logs',
      status,
      createdAt: 1,
      startedAt: 1,
      finishedAt: 2,
      durationMs: 1,
      exitCode: status === 'succeeded' ? 0 : 1,
      errorCategory: status === 'failed' ? 'permission' : null,
      errorMessage: status === 'failed' ? 'Permission denied' : null,
      retryable: false,
      parametersSummary: null,
      outputSummary: status === 'succeeded' ? '76 条日志记录' : null,
      remoteProcessGroup: null,
    },
    parameters: [],
    files: [],
  };
}

describe('LogSearchPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.listServers.mockResolvedValue([server]);
    apiMocks.searchLogs.mockImplementation(async (_serverId, _request, onEvent) => {
      onEvent({
        type: 'started',
        sequence: 1,
        emittedAt: 1,
        executionId: 'execution-1',
        startedAt: 1,
      });
      onEvent({
        type: 'finished',
        sequence: 2,
        emittedAt: 2,
        status: 'succeeded',
        exitCode: 0,
        durationMs: 1,
        result: { count: 76 },
      });
      return details();
    });
    apiMocks.readLogResultPage
      .mockResolvedValueOnce({
        items: [
          {
            path: '/var/log/app.log',
            lineNumber: 18,
            kind: 'match',
            timestamp: '2026-08-03T10:01:02',
            text: 'ERROR request failed',
          },
        ],
        nextCursor: '50',
      })
      .mockResolvedValueOnce({
        items: [
          {
            path: '/var/log/app.log',
            lineNumber: 81,
            kind: 'context',
            timestamp: null,
            text: 'retry completed',
          },
        ],
        nextCursor: null,
      });
    apiMocks.downloadLogResult.mockResolvedValue('downloads/app-errors.txt');
  });

  it('submits all filters, shows streamed match count, pages by cursor, and downloads inside data root', async () => {
    const user = userEvent.setup();
    render(<LogSearchPage />);

    await screen.findByRole('option', { name: '麒麟日志机' });
    await user.clear(screen.getByLabelText('日志路径'));
    await user.type(screen.getByLabelText('日志路径'), '/var/log/app.log');
    await user.type(screen.getByLabelText('关键词'), 'ERROR');
    await user.click(screen.getByLabelText('区分大小写'));
    await user.clear(screen.getByLabelText('上下文行数'));
    await user.type(screen.getByLabelText('上下文行数'), '3');
    await user.clear(screen.getByLabelText('结果上限'));
    await user.type(screen.getByLabelText('结果上限'), '500');
    await user.type(screen.getByLabelText('开始时间'), '2026-08-03T10:00');
    await user.type(screen.getByLabelText('结束时间'), '2026-08-03T11:00');
    await user.click(screen.getByRole('button', { name: '开始检索' }));

    await waitFor(() => expect(apiMocks.searchLogs).toHaveBeenCalledWith(
      'server-1',
      {
        path: '/var/log/app.log',
        keyword: 'ERROR',
        caseSensitive: true,
        contextLines: 3,
        limit: 500,
        startTime: '2026-08-03T10:00',
        endTime: '2026-08-03T11:00',
      },
      expect.any(Function),
    ));
    expect(await screen.findByText('共匹配 76 条')).toBeVisible();
    expect(screen.getByText('ERROR request failed')).toBeVisible();
    expect(apiMocks.readLogResultPage).toHaveBeenCalledWith('execution-1', null, 50);

    await user.click(screen.getByRole('button', { name: '下一页' }));
    expect(await screen.findByText('retry completed')).toBeVisible();
    expect(apiMocks.readLogResultPage).toHaveBeenLastCalledWith('execution-1', '50', 50);
    expect(apiMocks.searchLogs).toHaveBeenCalledTimes(1);

    await user.clear(screen.getByLabelText('下载文件名'));
    await user.type(screen.getByLabelText('下载文件名'), 'app-errors.txt');
    await user.click(screen.getByRole('button', { name: '下载结果' }));
    expect(await screen.findByText('已保存到 downloads/app-errors.txt')).toBeVisible();
  });

  it('shows an empty state and classified permission guidance without raw stacks', async () => {
    const user = userEvent.setup();
    apiMocks.searchLogs.mockResolvedValue(details('failed'));
    apiMocks.readLogResultPage.mockResolvedValue({ items: [], nextCursor: null });
    render(<LogSearchPage />);

    await screen.findByRole('option', { name: '麒麟日志机' });
    await user.type(screen.getByLabelText('关键词'), 'ERROR');
    await user.click(screen.getByRole('button', { name: '开始检索' }));

    expect(await screen.findByText('远端账号无权读取该日志，请检查文件权限或更换账号。')).toBeVisible();
    expect(screen.queryByText(/stack/i)).not.toBeInTheDocument();
    expect(apiMocks.readLogResultPage).not.toHaveBeenCalled();
  });
});
