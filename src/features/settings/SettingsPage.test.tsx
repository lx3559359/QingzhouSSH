import '@testing-library/jest-dom/vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { UpdateStatus } from '../../api/contracts';

const apiMocks = vi.hoisted(() => ({
  getUpdateStatus: vi.fn(),
  setAutoUpdateCheck: vi.fn(),
  checkForUpdate: vi.fn(),
}));

vi.mock('../../api/tauri', () => ({ api: apiMocks }));

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

describe('SettingsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.getUpdateStatus.mockResolvedValue(availableStatus);
    apiMocks.setAutoUpdateCheck.mockImplementation(async (enabled: boolean) => ({
      ...availableStatus,
      autoCheck: enabled,
    }));
    apiMocks.checkForUpdate.mockResolvedValue(availableStatus);
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
});
