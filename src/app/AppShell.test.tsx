import '@testing-library/jest-dom/vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

vi.mock('../features/servers/ServerListPage', () => ({
  ServerListPage: () => <section aria-label="服务器内容">服务器内容</section>,
}));
vi.mock('../features/tasks/TaskPage', () => ({
  TaskPage: () => <section aria-label="快捷任务内容">快捷任务内容</section>,
}));
vi.mock('../features/logs/LogSearchPage', () => ({
  LogSearchPage: () => <section aria-label="日志检索内容">日志检索内容</section>,
}));
vi.mock('../features/transfers/FileTransferPage', () => ({
  FileTransferPage: () => <section aria-label="文件传输内容">文件传输内容</section>,
}));
vi.mock('../features/downloads/DownloadsPage', () => ({
  DownloadsPage: () => <section aria-label="下载文件内容">下载文件内容</section>,
}));
vi.mock('../features/history/ExecutionHistoryPage', () => ({
  ExecutionHistoryPage: () => <section aria-label="执行记录内容">执行记录内容</section>,
}));
vi.mock('../features/workflows/WorkflowPage', () => ({
  WorkflowPage: () => <section aria-label="工作流内容">工作流内容</section>,
}));
vi.mock('../features/settings/SettingsPage', () => ({
  SettingsPage: () => <section aria-label="设置内容">设置内容</section>,
}));

import { AppShell } from './AppShell';

describe('AppShell', () => {
  it('offers the complete navigation and opens the workflow builder', async () => {
    const user = userEvent.setup();
    render(<AppShell dataRoot="D:\\Codex Project\\轻量化SSH快捷工具\\data" />);

    for (const name of ['首页', '服务器', '快捷任务', '日志检索', '文件传输', '工作流', '执行记录', '下载文件', '设置']) {
      expect(screen.getByRole('button', { name })).toBeVisible();
    }

    await user.click(screen.getByRole('button', { name: '快捷任务' }));
    expect(screen.getByLabelText('快捷任务内容')).toBeVisible();

    await user.click(screen.getByRole('button', { name: '日志检索' }));
    expect(screen.getByLabelText('日志检索内容')).toBeVisible();

    await user.click(screen.getByRole('button', { name: '工作流' }));
    expect(screen.getByLabelText('工作流内容')).toBeVisible();
    expect(screen.queryByText('工作流将在下一里程碑开放')).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: '设置' }));
    expect(screen.getByLabelText('设置内容')).toBeVisible();
  });
});
