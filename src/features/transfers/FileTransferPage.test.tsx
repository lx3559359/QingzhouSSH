import '@testing-library/jest-dom/vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  listServers: vi.fn(),
  listLocalDirectory: vi.fn(),
  listRemoteDirectory: vi.fn(),
  uploadFile: vi.fn(),
  downloadFile: vi.fn(),
  cancelExecution: vi.fn(),
  enqueueUploadFile: vi.fn(),
  enqueueDownloadFile: vi.fn(),
  listTransferJobs: vi.fn(),
  cancelTransferJob: vi.fn(),
  retryTransferJob: vi.fn(),
}));
vi.mock('../../api/tauri', () => ({ api: apiMocks }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

import type { TransferJob } from '../../api/contracts';
import { directorySessionCache } from '../file-browser/directorySessionCache';
import { FileTransferPage } from './FileTransferPage';

const server = { id: 'server-1', name: 'UOS 文件机', host: '10.0.0.2', port: 22, username: 'ops', authKind: 'password' as const, credentialId: 'credential-1' };
const localListing = {
  path: 'D:\\project',
  parent: 'D:\\',
  entries: [
    { name: 'downloads', path: 'D:\\project\\downloads', kind: 'directory' as const, size: null, modifiedAt: null },
    { name: 'upload.bin', path: 'D:\\project\\upload.bin', kind: 'file' as const, size: 2048, modifiedAt: 1_700_000_000 },
  ],
};
const remoteRoot = {
  path: '/',
  parent: null,
  entries: [{ name: 'srv', path: '/srv', kind: 'directory' as const, size: null, modifiedAt: null }],
};
const remoteSrv = {
  path: '/srv',
  parent: '/',
  entries: [{ name: 'report.log', path: '/srv/report.log', kind: 'file' as const, size: 4096, modifiedAt: 1_700_000_100 }],
};

let transferJobs: TransferJob[] = [];

function transferJob(direction: 'upload' | 'download', status: TransferJob['status'] = 'succeeded'): TransferJob {
  return {
    id: `transfer-${transferJobs.length + 1}`,
    executionId: status === 'queued' ? null : 'execution-1',
    serverId: server.id,
    direction,
    sourcePath: direction === 'upload' ? 'D:\\project\\upload.bin' : '/srv/report.log',
    targetPath: direction === 'upload' ? '/srv/upload.bin' : 'report.log',
    overwrite: false,
    verification: 'balanced',
    status,
    transferred: status === 'succeeded' ? 2048 : 1024,
    total: 2048,
    percent: status === 'succeeded' ? 100 : 50,
    bytesPerSecond: 2048,
    averageBytesPerSecond: 1024,
    etaSeconds: status === 'succeeded' ? 0 : 1,
    attemptCount: 1,
    maxAttempts: 3,
    cancelRequested: false,
    retryable: status === 'failed',
    errorCategory: status === 'failed' ? 'io' : null,
    errorMessage: status === 'failed' ? 'connection reset' : null,
    sha256: status === 'succeeded' ? 'a'.repeat(64) : null,
    location: status === 'succeeded' ? (direction === 'download' ? 'downloads/report.log' : '/srv/upload.bin') : null,
    createdAt: 1,
    updatedAt: 2,
    startedAt: status === 'queued' ? null : 1,
    finishedAt: ['succeeded', 'failed', 'cancelled', 'uncertain'].includes(status) ? 2 : null,
  };
}

describe('FileTransferPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    transferJobs = [];
    directorySessionCache.clear();
    apiMocks.listServers.mockResolvedValue([server]);
    apiMocks.listLocalDirectory.mockResolvedValue(localListing);
    apiMocks.listRemoteDirectory.mockImplementation(async (_serverId, path) => path === '/srv' ? remoteSrv : remoteRoot);
    apiMocks.listTransferJobs.mockImplementation(async () => [...transferJobs]);
    apiMocks.cancelTransferJob.mockImplementation(async (jobId) => {
      const job = transferJobs.find((item) => item.id === jobId)!;
      job.status = 'cancelled';
      job.cancelRequested = true;
      return job;
    });
    apiMocks.retryTransferJob.mockImplementation(async (jobId) => {
      const job = transferJobs.find((item) => item.id === jobId)!;
      job.status = 'queued';
      job.errorCategory = null;
      job.errorMessage = null;
      return job;
    });
    apiMocks.enqueueUploadFile.mockImplementation(async () => {
      const job = transferJob('upload');
      transferJobs = [job, ...transferJobs];
      return job;
    });
    apiMocks.enqueueDownloadFile.mockImplementation(async () => {
      const job = transferJob('download');
      transferJobs = [job, ...transferJobs];
      return job;
    });
  });

  it('reuses a remote directory after the page is reopened in the same session', async () => {
    const firstView = render(<FileTransferPage />);
    expect(await screen.findByRole('button', { name: '打开远程目录 srv' })).toBeVisible();
    firstView.unmount();

    render(<FileTransferPage />);
    expect(await screen.findByRole('button', { name: '打开远程目录 srv' })).toBeVisible();

    expect(apiMocks.listRemoteDirectory).toHaveBeenCalledTimes(1);
  });

  it('keeps the current remote rows visible while an explicit refresh is running', async () => {
    const user = userEvent.setup();
    let finishRefresh!: (value: typeof remoteRoot) => void;
    apiMocks.listRemoteDirectory
      .mockResolvedValueOnce(remoteRoot)
      .mockImplementationOnce(() => new Promise((resolve) => { finishRefresh = resolve; }));
    render(<FileTransferPage />);

    const remotePane = await screen.findByRole('region', { name: '远程文件浏览器' });
    expect(await within(remotePane).findByRole('button', { name: '打开远程目录 srv' })).toBeVisible();
    await user.click(within(remotePane).getByRole('button', { name: '刷新' }));

    expect(within(remotePane).getByRole('button', { name: '打开远程目录 srv' })).toBeVisible();
    expect(within(remotePane).getByRole('status')).toHaveTextContent('正在刷新');
    finishRefresh(remoteRoot);
    await waitFor(() => expect(within(remotePane).queryByRole('status')).not.toBeInTheDocument());
    expect(apiMocks.listRemoteDirectory).toHaveBeenCalledTimes(2);
  });

  it('keeps the latest path visible when an older refresh finishes late', async () => {
    const user = userEvent.setup();
    const slow = {
      path: '/slow',
      parent: '/',
      entries: [{ name: 'fast', path: '/fast', kind: 'directory' as const, size: null, modifiedAt: null }],
    };
    const fast = {
      path: '/fast',
      parent: '/',
      entries: [{ name: 'winner.txt', path: '/fast/winner.txt', kind: 'file' as const, size: 1, modifiedAt: null }],
    };
    const rootWithSlow = {
      ...remoteRoot,
      entries: [{ name: 'slow', path: '/slow', kind: 'directory' as const, size: null, modifiedAt: null }],
    };
    let finishSlowRefresh!: (value: typeof slow) => void;
    apiMocks.listRemoteDirectory.mockImplementation(async (_serverId, path) => {
      if (path === '/') return rootWithSlow;
      if (path === '/fast') return fast;
      return slow;
    });
    render(<FileTransferPage />);
    const remotePane = await screen.findByRole('region', { name: '远程文件浏览器' });

    await user.click(await within(remotePane).findByRole('button', { name: '打开远程目录 slow' }));
    expect(await within(remotePane).findByRole('button', { name: '打开远程目录 fast' })).toBeVisible();
    apiMocks.listRemoteDirectory.mockImplementation(async (_serverId, path) => {
      if (path === '/slow') {
        return new Promise<typeof slow>((resolve) => { finishSlowRefresh = resolve; });
      }
      if (path === '/fast') return fast;
      return rootWithSlow;
    });
    await user.click(within(remotePane).getByRole('button', { name: '刷新' }));
    await user.click(within(remotePane).getByRole('button', { name: '打开远程目录 fast' }));

    expect(await within(remotePane).findByRole('button', { name: '选择远程文件 winner.txt' })).toBeVisible();
    await act(async () => {
      finishSlowRefresh(slow);
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(within(remotePane).getByText('/fast')).toBeVisible();
      expect(within(remotePane).getByRole('button', { name: '选择远程文件 winner.txt' })).toBeVisible();
    });
  });

  it('does not reserve a status panel until a transfer has details to show', async () => {
    render(<FileTransferPage />);

    await screen.findByRole('region', { name: '本地文件浏览器' });
    expect(screen.queryByRole('region', { name: '传输状态' })).not.toBeInTheDocument();
    expect(screen.queryByText('等待选择文件')).not.toBeInTheDocument();
    expect(screen.queryByText('来源')).not.toBeInTheDocument();
    expect(screen.queryByText('目标')).not.toBeInTheDocument();
    expect(screen.queryByText('已传输')).not.toBeInTheDocument();
  });

  it('uses two browsable panes and derives a safe upload target from selections', async () => {
    const user = userEvent.setup();
    render(<FileTransferPage />);

    expect(await screen.findByRole('region', { name: '本地文件浏览器' })).toBeVisible();
    expect(await screen.findByRole('region', { name: '远程文件浏览器' })).toBeVisible();
    expect(apiMocks.listLocalDirectory).toHaveBeenCalledWith(null);
    expect(apiMocks.listRemoteDirectory).toHaveBeenCalledWith('server-1', '/');

    await user.click(screen.getByRole('button', { name: '选择本地文件 upload.bin' }));
    await user.click(await screen.findByRole('button', { name: '打开远程目录 srv' }));
    await user.click(screen.getByRole('button', { name: '上传到右侧目录' }));

    expect(apiMocks.enqueueUploadFile).toHaveBeenCalledWith(
      'server-1',
      {
        localPath: 'D:\\project\\upload.bin',
        remotePath: '/srv/upload.bin',
        overwrite: false,
        verification: 'balanced',
      },
    );
    expect(await screen.findByText('2 KB / 2 KB')).toBeVisible();
    expect(screen.getByText('实时状态 · 已完成')).toBeVisible();
    expect(screen.getByText('2 KB/s')).toBeVisible();
    expect(screen.getByText('1 KB/s')).toBeVisible();
    expect(within(screen.getByRole('region', { name: '传输状态' })).getByText('已完成')).toBeVisible();
    expect(screen.getByText('SHA-256 已校验')).toBeVisible();
  });

  it('joins an upload target at a Windows SFTP drive root without a duplicate slash', async () => {
    apiMocks.listRemoteDirectory.mockResolvedValue({ path: 'C:/', parent: null, entries: [] });
    const user = userEvent.setup();
    render(<FileTransferPage />);

    await user.click(await screen.findByRole('button', { name: '选择本地文件 upload.bin' }));
    await user.click(screen.getByRole('button', { name: '上传到右侧目录' }));

    expect(apiMocks.enqueueUploadFile).toHaveBeenCalledWith(
      'server-1',
      expect.objectContaining({ remotePath: 'C:/upload.bin' }),
    );
  });

  it('downloads a selected remote file only to the project data directory', async () => {
    const user = userEvent.setup();
    render(<FileTransferPage />);
    await screen.findByRole('region', { name: '远程文件浏览器' });

    await user.click(await screen.findByRole('button', { name: '打开远程目录 srv' }));
    await user.click(await screen.findByRole('button', { name: '选择远程文件 report.log' }));
    await user.click(screen.getByRole('button', { name: '下载到项目目录' }));

    expect(apiMocks.enqueueDownloadFile).toHaveBeenCalledWith(
      'server-1',
      {
        remotePath: '/srv/report.log',
        suggestedName: 'report.log',
        overwrite: false,
        verification: 'balanced',
      },
    );
    await waitFor(() => expect(screen.getByText('downloads/report.log')).toBeVisible());
  });

  it('offers only safe object-specific context actions and uses the right-clicked remote file', async () => {
    const user = userEvent.setup();
    const searchRemoteFile = vi.fn();
    render(<FileTransferPage onSearchRemoteFile={searchRemoteFile} />);

    await screen.findByRole('button', { name: '打开远程目录 srv' });
    const localFile = await screen.findByRole('button', { name: '选择本地文件 upload.bin' });
    fireEvent.contextMenu(localFile, { clientX: 30, clientY: 40 });
    let menu = screen.getByRole('menu');
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
      '上传',
      '复制文件名',
      '复制完整路径',
    ]);
    expect(within(menu).queryByText(/删除|重命名|新建/)).not.toBeInTheDocument();
    fireEvent.keyDown(document, { key: 'Escape' });

    fireEvent.contextMenu(screen.getByRole('button', { name: '打开本地目录 downloads' }), { clientX: 30, clientY: 40 });
    menu = screen.getByRole('menu');
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
      '打开文件夹',
      '刷新当前目录',
      '复制完整路径',
    ]);
    fireEvent.keyDown(document, { key: 'Escape' });

    const remoteFolder = await screen.findByRole('button', { name: '打开远程目录 srv' });
    fireEvent.contextMenu(remoteFolder, { clientX: 50, clientY: 60 });
    menu = screen.getByRole('menu');
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
      '打开文件夹',
      '刷新当前目录',
      '复制完整路径',
    ]);
    fireEvent.keyDown(document, { key: 'Escape' });

    await user.click(remoteFolder);
    const remoteFile = await screen.findByRole('button', { name: '选择远程文件 report.log' });
    fireEvent.contextMenu(remoteFile, { clientX: 50, clientY: 60 });
    menu = screen.getByRole('menu');
    expect(within(menu).getAllByRole('menuitem').map((item) => item.textContent)).toEqual([
      '下载',
      '搜索文件内容',
      '复制文件名',
      '复制完整路径',
    ]);
    await user.click(within(menu).getByRole('menuitem', { name: '下载' }));

    expect(apiMocks.enqueueDownloadFile).toHaveBeenCalledWith(
      'server-1',
      {
        remotePath: '/srv/report.log',
        suggestedName: 'report.log',
        overwrite: false,
        verification: 'balanced',
      },
    );
  });

  it('cancels queued work immediately and offers bounded retry for failed work', async () => {
    const queued = transferJob('upload', 'queued');
    const failed = transferJob('download', 'failed');
    failed.id = 'transfer-failed';
    transferJobs = [failed, queued];
    const user = userEvent.setup();
    render(<FileTransferPage />);

    const queue = await screen.findByRole('region', { name: '传输队列' });
    await user.click(within(queue).getByRole('button', { name: `取消 ${queued.sourcePath}` }));
    expect(apiMocks.cancelTransferJob).toHaveBeenCalledWith(queued.id);

    await user.click(within(queue).getByRole('button', { name: `重试 ${failed.sourcePath}` }));
    expect(apiMocks.retryTransferJob).toHaveBeenCalledWith(failed.id);
  });
});
