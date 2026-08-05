import '@testing-library/jest-dom/vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  listServers: vi.fn(),
  listRemoteDirectory: vi.fn(),
  searchLogs: vi.fn(),
  readLogResultPage: vi.fn(),
  downloadLogResult: vi.fn(),
  downloadFile: vi.fn(),
}));

vi.mock('../../api/tauri', () => ({ api: apiMocks }));

import type { ExecutionDetails } from '../../api/contracts';
import { directorySessionCache } from '../file-browser/directorySessionCache';
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
    directorySessionCache.clear();
    apiMocks.listServers.mockResolvedValue([server]);
    apiMocks.listRemoteDirectory.mockResolvedValue([]);
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
      .mockImplementation(async (_executionId, cursor) => cursor === '50' ? ({
        items: [
          {
            resultType: 'content',
            path: '/var/log/app.log',
            lineNumber: 81,
            kind: 'context',
            timestamp: null,
            text: 'retry completed',
          },
        ],
        nextCursor: null,
      }) : ({
        items: [
          {
            resultType: 'content',
            path: '/var/log/app.log',
            lineNumber: 18,
            kind: 'match',
            timestamp: '2026-08-03T10:01:02',
            text: 'ERROR request failed',
          },
        ],
        nextCursor: '50',
      }));
    apiMocks.downloadLogResult.mockResolvedValue('downloads/app-errors.txt');
    apiMocks.downloadFile.mockResolvedValue({
      ...details(),
      record: { ...details().record, taskId: 'transfer.download', outputSummary: '下载完成' },
      files: [{ id: 'file-1', relativePath: 'downloads/requirements.txt', purpose: 'download', sizeBytes: 96, sha256: 'a'.repeat(64) }],
    });
  });

  it('reopens the same remote log directory from session cache', async () => {
    const user = userEvent.setup();
    apiMocks.listRemoteDirectory.mockResolvedValue({
      path: '/var/log',
      parent: '/var',
      entries: [
        { name: 'messages', path: '/var/log/messages', kind: 'file', size: 2048, modifiedAt: 1_700_000_000 },
      ],
    });
    render(<LogSearchPage />);

    await screen.findByRole('option', { name: '麒麟日志机' });
    await user.click(screen.getByRole('radio', { name: '指定日志文件' }));
    await user.click(screen.getByRole('button', { name: '浏览服务器' }));
    expect(await screen.findByRole('button', { name: '选择日志 messages' })).toBeVisible();
    await user.click(screen.getByRole('button', { name: '关闭远程日志选择' }));

    await user.click(screen.getByRole('button', { name: '浏览服务器' }));
    expect(await screen.findByRole('button', { name: '选择日志 messages' })).toBeVisible();
    expect(apiMocks.listRemoteDirectory).toHaveBeenCalledTimes(1);
  });

  it('searches common logs by keyword without asking a novice for a path', async () => {
    const user = userEvent.setup();
    render(<LogSearchPage />);

    await screen.findByRole('option', { name: '麒麟日志机' });
    expect(screen.getByRole('radio', { name: '智能搜索（推荐）' })).toBeChecked();
    expect(screen.queryByLabelText('日志路径')).not.toBeInTheDocument();
    expect(screen.getByText('不需要知道日志路径')).toBeVisible();

    await user.type(screen.getByLabelText('搜索内容'), '连接超时');
    await user.click(screen.getByRole('button', { name: '开始检索' }));

    await waitFor(() => expect(apiMocks.searchLogs).toHaveBeenCalledWith(
      'server-1',
      expect.objectContaining({ path: '', keyword: '连接超时' }),
      expect.any(Function),
    ));
  });

  it('finds remote files by a fuzzy filename without exposing content-only controls', async () => {
    const user = userEvent.setup();
    apiMocks.readLogResultPage.mockResolvedValueOnce({
      items: [
        {
          resultType: 'file',
          path: '/home/app/requirements.txt',
          name: 'requirements.txt',
          size: 96,
          modifiedAt: 1_785_801_600,
        },
      ],
      nextCursor: null,
    });
    render(<LogSearchPage />);

    await screen.findByRole('option', { name: '麒麟日志机' });
    await user.click(screen.getByRole('radio', { name: '找文件名' }));

    expect(screen.getByLabelText('文件名包含')).toHaveAttribute('placeholder', expect.stringContaining('requi'));
    expect(screen.queryByLabelText('日志路径')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('开始时间')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('上下文行数')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('区分大小写')).not.toBeInTheDocument();

    await user.type(screen.getByLabelText('文件名包含'), 'requi');
    await user.click(screen.getByRole('button', { name: '开始查找' }));

    await waitFor(() => expect(apiMocks.searchLogs).toHaveBeenCalledWith(
      'server-1',
      {
        target: 'filename',
        path: '',
        keyword: 'requi',
        caseSensitive: false,
        contextLines: 0,
        limit: 200,
        startTime: null,
        endTime: null,
      },
      expect.any(Function),
    ));
    expect(await screen.findByText('requirements.txt')).toBeVisible();
    expect(screen.getByText('/home/app/requirements.txt')).toBeVisible();
    expect(screen.getByText('96 B')).toBeVisible();
    expect(screen.queryByText(/第 1 行/)).not.toBeInTheDocument();
  });

  it('submits all filters, shows streamed match count, pages by cursor, and downloads inside data root', async () => {
    const user = userEvent.setup();
    render(<LogSearchPage />);

    await screen.findByRole('option', { name: '麒麟日志机' });
    await user.click(screen.getByRole('radio', { name: '指定日志文件' }));
    await user.clear(screen.getByLabelText('日志路径'));
    await user.type(screen.getByLabelText('日志路径'), '/var/log/app.log');
    await user.type(screen.getByLabelText('搜索内容'), 'ERROR');
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
        target: 'content',
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
    await user.type(screen.getByLabelText('搜索内容'), 'ERROR');
    await user.click(screen.getByRole('button', { name: '开始检索' }));

    expect(await screen.findByText('远端账号无权读取该日志，请检查文件权限或更换账号。')).toBeVisible();
    expect(screen.queryByText(/stack/i)).not.toBeInTheDocument();
    expect(apiMocks.readLogResultPage).not.toHaveBeenCalled();
  });

  it('browses remote directories and selects a log without requiring a known path', async () => {
    const user = userEvent.setup();
    apiMocks.listRemoteDirectory
      .mockResolvedValueOnce({
        path: '/var/log',
        parent: '/var',
        entries: [
          { name: 'nginx', path: '/var/log/nginx', kind: 'directory', size: null, modifiedAt: null },
          { name: 'messages', path: '/var/log/messages', kind: 'file', size: 2048, modifiedAt: 1_700_000_000 },
        ],
      })
      .mockResolvedValueOnce({
        path: '/var/log/nginx',
        parent: '/var/log',
        entries: [
          { name: 'access.log', path: '/var/log/nginx/access.log', kind: 'file', size: 4096, modifiedAt: 1_700_000_100 },
        ],
      });
    render(<LogSearchPage />);

    await screen.findByRole('option', { name: '麒麟日志机' });
    await user.click(screen.getByRole('radio', { name: '指定日志文件' }));
    await user.click(screen.getByRole('button', { name: '浏览服务器' }));
    expect(await screen.findByRole('dialog', { name: '选择远程日志' })).toBeVisible();
    expect(apiMocks.listRemoteDirectory).toHaveBeenCalledWith('server-1', '/var/log');

    await user.click(screen.getByRole('button', { name: '打开目录 nginx' }));
    expect(apiMocks.listRemoteDirectory).toHaveBeenLastCalledWith('server-1', '/var/log/nginx');
    await user.click(await screen.findByRole('button', { name: '选择日志 access.log' }));

    expect(screen.getByLabelText('日志路径')).toHaveValue('/var/log/nginx/access.log');
    expect(screen.queryByRole('dialog', { name: '选择远程日志' })).not.toBeInTheDocument();
  });

  it('offers safe actions for filename results and can switch one result to content search', async () => {
    const user = userEvent.setup();
    apiMocks.readLogResultPage.mockResolvedValue({
      items: [{ resultType: 'file', path: '/home/app/requirements.txt', name: 'requirements.txt', size: 96, modifiedAt: null }],
      nextCursor: null,
    });
    render(<LogSearchPage />);

    await screen.findByRole('option', { name: '麒麟日志机' });
    await user.click(screen.getByRole('radio', { name: '找文件名' }));
    await user.type(screen.getByLabelText('文件名包含'), 'requi');
    await user.click(screen.getByRole('button', { name: '开始查找' }));
    const resultRow = (await screen.findByText('requirements.txt')).closest('tr');
    expect(resultRow).not.toBeNull();

    fireEvent.contextMenu(resultRow!, { clientX: 100, clientY: 100 });
    let menu = screen.getByRole('menu');
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
      '下载文件',
      '搜索文件内容',
      '复制完整路径',
    ]);
    expect(within(menu).queryByText(/删除|重命名|新建/)).not.toBeInTheDocument();
    await user.click(within(menu).getByRole('menuitem', { name: '下载文件' }));
    expect(apiMocks.downloadFile).toHaveBeenCalledWith(
      'server-1',
      { remotePath: '/home/app/requirements.txt', suggestedName: 'requirements.txt', overwrite: false },
      expect.any(Function),
    );

    fireEvent.contextMenu(resultRow!, { clientX: 100, clientY: 100 });
    menu = screen.getByRole('menu');
    await user.click(within(menu).getByRole('menuitem', { name: '搜索文件内容' }));

    expect(screen.getByRole('radio', { name: '搜日志内容' })).toBeChecked();
    expect(screen.getByRole('radio', { name: '指定日志文件' })).toBeChecked();
    expect(screen.getByLabelText('日志路径')).toHaveValue('/home/app/requirements.txt');
    expect(screen.getByLabelText('搜索内容')).toHaveValue('requi');
  });

  it('offers only copy actions for a content result row', async () => {
    const user = userEvent.setup();
    render(<LogSearchPage />);
    await screen.findByRole('option', { name: '麒麟日志机' });
    await user.type(screen.getByLabelText('搜索内容'), 'ERROR');
    await user.click(screen.getByRole('button', { name: '开始检索' }));
    const resultRow = (await screen.findByText('ERROR request failed')).closest('tr');
    fireEvent.contextMenu(resultRow!, { clientX: 100, clientY: 100 });

    const menu = screen.getByRole('menu');
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
      '复制本行',
      '复制日志路径',
    ]);
    expect(within(menu).queryByText(/删除|重命名|新建|下载/)).not.toBeInTheDocument();
  });
});
