import '@testing-library/jest-dom/vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({ listServers: vi.fn(), uploadFile: vi.fn(), downloadFile: vi.fn(), cancelExecution: vi.fn() }));
vi.mock('../../api/tauri', () => ({ api: apiMocks }));

import type { ExecutionDetails } from '../../api/contracts';
import { FileTransferPage } from './FileTransferPage';

const server = { id: 'server-1', name: 'UOS 文件机', host: '10.0.0.2', port: 22, username: 'ops', authKind: 'password' as const, credentialId: 'credential-1' };
function result(taskId: string, status: 'succeeded' | 'cancelled' = 'succeeded'): ExecutionDetails {
  return { record: { id: 'transfer-1', serverId: server.id, taskId, taskVersion: 1, category: 'transfer', status, createdAt: 1, startedAt: 1, finishedAt: 2, durationMs: 1000, exitCode: null, errorCategory: status === 'cancelled' ? 'cancelled' : null, errorMessage: null, retryable: false, parametersSummary: null, outputSummary: status === 'succeeded' ? `传输 2048 字节，SHA-256 ${'a'.repeat(64)}` : null, remoteProcessGroup: null }, parameters: [], files: taskId.endsWith('download') && status === 'succeeded' ? [{ id: 'file-1', relativePath: 'downloads/report.log', purpose: 'download', sizeBytes: 2048, sha256: 'a'.repeat(64) }] : [] };
}

describe('FileTransferPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.listServers.mockResolvedValue([server]);
    apiMocks.cancelExecution.mockResolvedValue(undefined);
    apiMocks.uploadFile.mockImplementation(async (_id, _request, onEvent) => {
      onEvent({ type: 'started', executionId: 'transfer-1', startedAt: 1000, sequence: 1, emittedAt: 1000 });
      onEvent({ type: 'progress', transferred: 1024, total: 2048, percent: 50, sequence: 2, emittedAt: 2000 });
      onEvent({ type: 'finished', status: 'succeeded', exitCode: null, durationMs: 1000, result: { bytes: 2048, sha256: 'a'.repeat(64), location: '/srv/upload.bin' }, sequence: 3, emittedAt: 2000 });
      return result('transfer.upload');
    });
    apiMocks.downloadFile.mockResolvedValue(result('transfer.download'));
  });

  it('shows local and remote paths, byte progress, speed, and SHA-256 verification', async () => {
    const user = userEvent.setup();
    render(<FileTransferPage />);
    await screen.findByRole('option', { name: 'UOS 文件机' });
    await user.type(screen.getByLabelText('上传本地路径'), 'D:\\project\\upload.bin');
    await user.type(screen.getByLabelText('上传远端路径'), '/srv/upload.bin');
    await user.click(screen.getByRole('button', { name: '开始上传' }));

    expect(apiMocks.uploadFile).toHaveBeenCalledWith('server-1', { localPath: 'D:\\project\\upload.bin', remotePath: '/srv/upload.bin', overwrite: false }, expect.any(Function));
    expect(await screen.findByText('1 KB / 2 KB')).toBeVisible();
    expect(screen.getByText('50%')).toBeVisible();
    expect(screen.getByText('1 KB/s')).toBeVisible();
    expect(screen.getByText('SHA-256 已校验')).toBeVisible();
    expect(screen.getByText('D:\\project\\upload.bin')).toBeVisible();
    expect(screen.getAllByText('/srv/upload.bin')).toHaveLength(2);
  });

  it('downloads only to a data-root relative location and never labels cancellation as success', async () => {
    const user = userEvent.setup();
    render(<FileTransferPage />);
    await screen.findByRole('option', { name: 'UOS 文件机' });
    await user.click(screen.getByRole('radio', { name: '下载' }));
    await user.type(screen.getByLabelText('下载远端路径'), '/var/log/report.log');
    await user.type(screen.getByLabelText('本地文件名'), 'report.log');
    await user.click(screen.getByRole('button', { name: '开始下载' }));
    await waitFor(() => expect(screen.getAllByText('downloads/report.log')).toHaveLength(2));

    apiMocks.downloadFile.mockResolvedValue(result('transfer.download', 'cancelled'));
    await user.click(screen.getByRole('button', { name: '开始下载' }));
    expect(await screen.findByText('传输已取消')).toBeVisible();
    expect(screen.queryByText('传输成功')).not.toBeInTheDocument();
  });
});
