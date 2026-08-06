import '@testing-library/jest-dom/vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { ReadyBootstrapStatus, UpdateStatus } from '../../api/contracts';

const apiMocks = vi.hoisted(() => ({
  getUpdateStatus: vi.fn(),
  setAutoUpdateCheck: vi.fn(),
  checkForUpdate: vi.fn(),
  downloadUpdate: vi.fn(),
  installUpdate: vi.fn(),
  clearDownloadedUpdate: vi.fn(),
  preflightDataRootMigration: vi.fn(),
  startDataRootMigration: vi.fn(),
  preflightRetryDataRootMigration: vi.fn(),
  preflightPortableDefaultDataRootMigration: vi.fn(),
}));

const dialogMocks = vi.hoisted(() => ({ open: vi.fn() }));

vi.mock('../../api/tauri', () => ({
  api: apiMocks,
  asAppError: (cause: unknown) =>
    typeof cause === 'object' && cause !== null && 'message' in cause
      ? cause
      : { code: 'unknown', message: '操作失败' },
}));
vi.mock('@tauri-apps/plugin-dialog', () => dialogMocks);

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
const bootstrap: ReadyBootstrapStatus = {
  state: 'ready',
  dataRoot,
  dataRootSource: 'registry',
  dataRootMutable: true,
  lastDataMigration: null,
};

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
    dialogMocks.open.mockResolvedValue(null);
    apiMocks.preflightDataRootMigration.mockResolvedValue({
      previewId: 'preview-1',
      confirmationToken: 'token-1',
      expiresAt: Date.now() + 300_000,
      source: dataRoot,
      target: String.raw`D:\QingzhouData-New`,
      fileCount: 128,
      totalBytes: 42 * 1024 * 1024,
      requiredBytes: 106 * 1024 * 1024,
      availableBytes: 300 * 1024 * 1024 * 1024,
      oldRootWillBeKept: true,
      retryable: false,
    });
    apiMocks.startDataRootMigration.mockResolvedValue({ migrationId: 'migration-1' });
    apiMocks.preflightRetryDataRootMigration.mockResolvedValue({
      previewId: 'retry-preview',
      confirmationToken: 'retry-token',
      expiresAt: Date.now() + 300_000,
      source: dataRoot,
      target: String.raw`D:\Failed-Target`,
      fileCount: 128,
      totalBytes: 42 * 1024 * 1024,
      requiredBytes: 106 * 1024 * 1024,
      availableBytes: 300 * 1024 * 1024 * 1024,
      oldRootWillBeKept: true,
      retryable: true,
    });
    apiMocks.preflightPortableDefaultDataRootMigration.mockResolvedValue({
      previewId: 'portable-preview',
      confirmationToken: 'portable-token',
      expiresAt: Date.now() + 300_000,
      source: dataRoot,
      target: String.raw`D:\QingzhouSSH\data`,
      fileCount: 128,
      totalBytes: 42 * 1024 * 1024,
      requiredBytes: 106 * 1024 * 1024,
      availableBytes: 300 * 1024 * 1024 * 1024,
      oldRootWillBeKept: true,
      retryable: false,
    });
  });

  it('shows version, project data root, update source, details and latest status', async () => {
    render(<SettingsPage bootstrap={bootstrap} />);

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
    render(<SettingsPage bootstrap={bootstrap} />);

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
    render(<SettingsPage bootstrap={bootstrap} />);

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
      render(<SettingsPage bootstrap={bootstrap} />);

      await user.click(await screen.findByRole('button', { name: '下载并验证' }));

      expect(await screen.findByText(message)).toBeVisible();
      expect(screen.getByRole('button', { name: '清理更新文件' })).toBeVisible();
    },
  );

  it('requires a second confirmation before installation and explains app exit', async () => {
    const user = userEvent.setup();
    apiMocks.getUpdateStatus.mockResolvedValue(downloadedStatus);
    render(<SettingsPage bootstrap={bootstrap} />);

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
    render(<SettingsPage bootstrap={bootstrap} />);

    await user.click(await screen.findByRole('button', { name: '清理更新文件' }));

    expect(apiMocks.clearDownloadedUpdate).toHaveBeenCalledOnce();
  });

  it('chooses a directory, previews the complete migration and requires confirmation', async () => {
    const user = userEvent.setup();
    dialogMocks.open.mockResolvedValue(String.raw`D:\QingzhouData-New`);
    render(<SettingsPage bootstrap={bootstrap} />);

    await user.click(await screen.findByRole('button', { name: '更改数据目录' }));
    const dialog = screen.getByRole('dialog', { name: '更改数据目录' });
    await user.click(within(dialog).getByRole('button', { name: '选择新的空文件夹' }));

    expect(apiMocks.preflightDataRootMigration).toHaveBeenCalledWith(String.raw`D:\QingzhouData-New`);
    expect(await within(dialog).findByText('128 个')).toBeVisible();
    expect(within(dialog).getByText(/旧目录不会删除或清空/)).toBeVisible();
    const start = within(dialog).getByRole('button', { name: '确认迁移并退出' });
    expect(start).toBeDisabled();
    await user.click(within(dialog).getByRole('checkbox'));
    await user.click(start);

    expect(apiMocks.startDataRootMigration).toHaveBeenCalledWith('preview-1', 'token-1');
    expect(await within(dialog).findByText('正在安全迁移数据，请等待客户端重新打开')).toBeVisible();
  });

  it('explains and disables client-side changes when the environment locks the root', async () => {
    render(<SettingsPage bootstrap={{ ...bootstrap, dataRootSource: 'environment', dataRootMutable: false }} />);
    expect(await screen.findByText('由 QINGZHOU_DATA_ROOT 环境变量锁定')).toBeVisible();
    expect(screen.getByRole('button', { name: '更改数据目录' })).toBeDisabled();
  });

  it('offers a verified retry for the last failed migration', async () => {
    const user = userEvent.setup();
    render(<SettingsPage bootstrap={{ ...bootstrap, lastDataMigration: failedMigration() }} />);
    await user.click(await screen.findByRole('button', { name: '重试上次迁移' }));
    const dialog = await screen.findByRole('dialog', { name: '重试数据迁移' });
    expect(apiMocks.preflightRetryDataRootMigration).toHaveBeenCalledOnce();
    expect(await within(dialog).findByText(/只会补传缺失或校验不同的文件/)).toBeVisible();
  });

  it('offers portable users a return to the data directory beside the program', async () => {
    const user = userEvent.setup();
    render(<SettingsPage bootstrap={{ ...bootstrap, dataRootSource: 'portable_custom' }} />);
    await user.click(await screen.findByRole('button', { name: '恢复程序旁目录' }));
    expect(apiMocks.preflightPortableDefaultDataRootMigration).toHaveBeenCalledOnce();
    expect(await screen.findByText(String.raw`D:\QingzhouSSH\data`)).toBeVisible();
  });
});

function failedMigration() {
  return {
    schemaVersion: 1,
    migrationId: 'failed-1',
    source: dataRoot,
    target: String.raw`D:\Failed-Target`,
    sourceMode: 'registry' as const,
    parentPid: 42,
    fileCount: 128,
    totalBytes: 42 * 1024 * 1024,
    copiedFiles: 10,
    copiedBytes: 1024,
    phase: 'failed' as const,
    errorSummary: '完整性校验失败',
    startedAt: 1,
    updatedAt: 2,
    acknowledged: false,
  };
}
