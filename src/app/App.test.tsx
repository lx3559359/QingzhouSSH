import '@testing-library/jest-dom/vitest';
import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  bootstrapStatus: vi.fn(),
  initializeDataRoot: vi.fn(),
}));

vi.mock('../api/tauri', () => ({ api: apiMocks }));
vi.mock('../features/servers/ServerListPage', () => ({
  ServerListPage: () => <section aria-label="服务器主页">服务器主页</section>,
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
    });
    render(<App />);

    expect(await screen.findByLabelText('服务器主页')).toBeVisible();
    expect(screen.getByText('D:\\QingzhouSSH')).toBeVisible();
  });
});
