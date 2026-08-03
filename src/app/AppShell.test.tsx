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

import { AppShell } from './AppShell';

describe('AppShell', () => {
  it('offers the complete navigation and labels workflow as the next milestone', async () => {
    const user = userEvent.setup();
    render(<AppShell />);

    for (const name of ['首页', '服务器', '快捷任务', '日志检索', '工作流', '执行记录', '下载文件', '设置']) {
      expect(screen.getByRole('button', { name })).toBeVisible();
    }

    await user.click(screen.getByRole('button', { name: '快捷任务' }));
    expect(screen.getByLabelText('快捷任务内容')).toBeVisible();

    await user.click(screen.getByRole('button', { name: '日志检索' }));
    expect(screen.getByLabelText('日志检索内容')).toBeVisible();

    await user.click(screen.getByRole('button', { name: '工作流' }));
    expect(screen.getByRole('heading', { name: '工作流将在下一里程碑开放' })).toBeVisible();
    expect(screen.queryByRole('button', { name: /运行工作流/ })).not.toBeInTheDocument();
  });
});
