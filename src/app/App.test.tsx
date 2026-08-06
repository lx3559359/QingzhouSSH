import '@testing-library/jest-dom/vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  bootstrapStatus: vi.fn(),
  initializeDataRoot: vi.fn(),
}));

const windowMocks = vi.hoisted(() => ({
  startDragging: vi.fn().mockResolvedValue(undefined),
  minimize: vi.fn().mockResolvedValue(undefined),
  toggleMaximize: vi.fn().mockResolvedValue(undefined),
  close: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('../api/tauri', () => ({ api: apiMocks }));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => windowMocks,
}));
vi.mock('./AppShell', () => ({
  AppShell: () => <section aria-label="操作中心">操作中心</section>,
}));

import { App } from './App';

describe('App', () => {
  beforeEach(() => vi.clearAllMocks());

  it('shows a branded loading state without exposing a terminal entry', () => {
    apiMocks.bootstrapStatus.mockReturnValue(new Promise(() => undefined));
    render(<App />);

    expect(screen.getByRole('heading', { name: '轻舟 SSH' })).toBeVisible();
    expect(screen.getByText('安全地完成 Linux 操作')).toBeVisible();
    expect(screen.getByText('正在准备本地环境…')).toBeVisible();
    expect(screen.queryByText('打开终端')).not.toBeInTheDocument();
  });

  it('shows the explicit data-root gate when no folder is configured', async () => {
    apiMocks.bootstrapStatus.mockResolvedValue({ state: 'needs_selection' });
    render(<App />);

    expect(
      await screen.findByRole('heading', { name: '选择数据存储位置' }),
    ).toBeVisible();
  });

  it('opens the server page when the project data root is ready', async () => {
    apiMocks.bootstrapStatus.mockResolvedValue({
      state: 'ready',
      dataRoot: 'D:\\QingzhouSSH',
      dataRootSource: 'registry',
      dataRootMutable: true,
      lastDataMigration: null,
    });
    render(<App />);

    expect(await screen.findByLabelText('操作中心')).toBeVisible();
    expect(screen.getByText('D:\\QingzhouSSH')).toBeVisible();
  });

  it('uses the blue header as the draggable frame with complete window controls', async () => {
    const user = userEvent.setup();
    apiMocks.bootstrapStatus.mockReturnValue(new Promise(() => undefined));
    render(<App />);

    const dragRegion = screen.getByTestId('window-drag-region');
    fireEvent.mouseDown(dragRegion, { button: 0 });
    expect(windowMocks.startDragging).toHaveBeenCalledTimes(1);

    fireEvent.mouseDown(dragRegion, { button: 2 });
    fireEvent.mouseDown(screen.getByRole('button', { name: '最小化窗口' }), { button: 0 });
    expect(windowMocks.startDragging).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole('button', { name: '最小化窗口' }));
    await user.click(screen.getByRole('button', { name: '最大化或还原窗口' }));
    await user.click(screen.getByRole('button', { name: '关闭窗口' }));

    expect(windowMocks.minimize).toHaveBeenCalledTimes(1);
    expect(windowMocks.toggleMaximize).toHaveBeenCalledTimes(1);
    expect(windowMocks.close).toHaveBeenCalledTimes(1);
  });
});
