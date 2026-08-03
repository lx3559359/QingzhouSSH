import '@testing-library/jest-dom/vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { UpdateStatus } from '../../api/contracts';

const apiMocks = vi.hoisted(() => ({
  getUpdateStatus: vi.fn(),
  setAutoUpdateCheck: vi.fn(),
  checkForUpdate: vi.fn(),
  downloadUpdate: vi.fn(),
  installUpdate: vi.fn(),
  clearDownloadedUpdate: vi.fn(),
}));

vi.mock('../../api/tauri', () => ({
  api: apiMocks,
  asAppError: (cause: unknown) =>
    typeof cause === 'object' && cause !== null && 'message' in cause
      ? cause
      : { code: 'unknown', message: '操作失败' },
}));

import { SettingsPage } from './SettingsPage';

const availableStatus: UpdateStatus = {
  currentVersion: '0.1.0',
  phase: 'available',
  autoCheck: true,
  lastCheckedAt: 1_785_812_400,
  lastResult: {
    status: 'available',
    version: '0.2.0',
    source: 'github',
    message: '发现可用更新',
  },
  release: {
    version: '0.2.0',
    notes: '增强国产 Linux 自动识别和日志检索下载。',
    publishedAt: '2026-08-04T10:00:00Z',
    size: 25_165_824,
    buildId: 'release-20260804',
    source: 'github',
    sourceLabel: 'GitHub Releases',
  },
  fallbackReason: null,
  staged: null,
  lastError: null,
};

const dataRoot = String.raw`D:\Codex Project\轻量化SSH快捷工具\data`;

const downloadedStatus: UpdateStatus = {
  ...availableStatus,
  phase: 'downloaded',
  staged: {
    version: '0.2.0',
    relativePath: 'staged/0.2.0/QingzhouSSH.nsis',
    sha256: 'a'.repeat(64),
    size: availableStatus.release!.size,
  },
};

describe('SettingsPage', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    apiMocks.getUpdateStatus.mockResolvedValue(availableStatus);
    apiMocks.setAutoUpdateCheck.mockImplementation(async (enabled: boolean) => ({
      ...availableStatus,
      autoCheck: enabled,
    }));
    apiMocks.checkForUpdate.mockResolvedValue(availableStatus);
    apiMocks.downloadUpdate.mockResolvedValue(downloadedStatus);
    apiMocks.installUpdate.mockResolvedValue({ ...downloadedStatus, phase: 'installing' });
    apiMocks.clearDownloadedUpdate.mockResolvedValue({
      ...availableStatus,
      phase: 'idle',
      release: null,
      staged: null,
    });
  });

  it('shows version, project data root, update source, details and latest status', async () => {
    render(<SettingsPage dataRoot={dataRoot} />);

    expect(await screen.findByRole('heading', { name: '设置与更新' })).toBeVisible();
    expect(screen.getByText('v0.1.0')).toBeVisible();
    expect(screen.getByText(dataRoot)).toBeVisible();
    expect(screen.getByText('GitHub Releases')).toBeVisible();
    expect(screen.getByText('发现可用更新')).toBeVisible();
    expect(screen.getByRole('heading', { name: '版本 0.2.0' })).toBeVisible();
    expect(screen.getByText('增强国产 Linux 自动识别和日志检索下载。')).toBeVisible();
  });

  it('persists the automatic update-check preference', async () => {
    const user = userEvent.setup();
    render(<SettingsPage dataRoot={dataRoot} />);

    const toggle = await screen.findByRole('checkbox', { name: '自动检查更新' });
    expect(toggle).toBeChecked();
    await user.click(toggle);

    expect(apiMocks.setAutoUpdateCheck).toHaveBeenCalledWith(false);
    expect(toggle).not.toBeChecked();
  });

  it('shows mirror fallback and monotonic download progress', async () => {
    const user = userEvent.setup();
    const mirrorStatus: UpdateStatus = {
      ...availableStatus,
      release: { ...availableStatus.release!, source: 'modelscope', sourceLabel: 'ModelScope 国内镜像' },
      fallbackReason: 'GitHub 暂时不可用，已切换国内镜像。',
    };
    apiMocks.getUpdateStatus.mockResolvedValue(mirrorStatus);
    apiMocks.downloadUpdate.mockImplementation(async (onEvent) => {
      onEvent({ sequence: 1, downloadedBytes: 5, totalBytes: 10 });
      onEvent({ sequence: 2, downloadedBytes: 10, totalBytes: 10 });
      return { ...downloadedStatus, release: mirrorStatus.release, fallbackReason: mirrorStatus.fallbackReason };
    });
    render(<SettingsPage dataRoot={dataRoot} />);

    expect(await screen.findByText('GitHub 暂时不可用，已切换国内镜像。')).toBeVisible();
    await user.click(screen.getByRole('button', { name: '下载并验证' }));

    expect(apiMocks.downloadUpdate).toHaveBeenCalledOnce();
    expect(await screen.findByText('100%')).toBeVisible();
    expect(screen.getByRole('button', { name: '安装更新' })).toBeVisible();
  });

  it.each(['更新签名验证失败', '更新包完整性校验失败'])(
    'blocks rejected downloads and offers cleanup: %s',
    async (message) => {
      const user = userEvent.setup();
      apiMocks.downloadUpdate.mockRejectedValue({ code: 'update', message });
      apiMocks.getUpdateStatus
        .mockResolvedValueOnce(availableStatus)
        .mockResolvedValueOnce({ ...availableStatus, phase: 'failed', lastError: message });
      render(<SettingsPage dataRoot={dataRoot} />);

      await user.click(await screen.findByRole('button', { name: '下载并验证' }));

      expect(await screen.findByText(message)).toBeVisible();
      expect(screen.getByRole('button', { name: '清理更新文件' })).toBeVisible();
    },
  );

  it('requires a second confirmation before installation and explains app exit', async () => {
    const user = userEvent.setup();
    apiMocks.getUpdateStatus.mockResolvedValue(downloadedStatus);
    render(<SettingsPage dataRoot={dataRoot} />);

    await user.click(await screen.findByRole('button', { name: '安装更新' }));
    const dialog = screen.getByRole('dialog', { name: '确认安装更新' });
    expect(dialog).toHaveTextContent('轻舟 SSH 将退出');
    expect(apiMocks.installUpdate).not.toHaveBeenCalled();

    await user.click(screen.getByRole('button', { name: '确认安装并退出' }));
    expect(apiMocks.installUpdate).toHaveBeenCalledWith(true);
  });

  it('clears a downloaded update on explicit request', async () => {
    const user = userEvent.setup();
    apiMocks.getUpdateStatus.mockResolvedValue(downloadedStatus);
    render(<SettingsPage dataRoot={dataRoot} />);

    await user.click(await screen.findByRole('button', { name: '清理更新文件' }));

    expect(apiMocks.clearDownloadedUpdate).toHaveBeenCalledOnce();
  });
});
