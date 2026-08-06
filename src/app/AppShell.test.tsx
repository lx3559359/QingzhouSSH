import '@testing-library/jest-dom/vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  acknowledgeDataRootMigration: vi.fn(),
  openDataRootFolder: vi.fn(),
}));

vi.mock('../api/tauri', () => ({
  api: apiMocks,
  asAppError: (cause: unknown) => cause instanceof Error ? cause : new Error(String(cause)),
}));

vi.mock('../features/servers/ServerListPage', () => ({
  ServerListPage: () => <section aria-label="服务器内容">服务器内容</section>,
}));
vi.mock('../features/tasks/TaskPage', () => ({
  TaskPage: () => <section aria-label="快捷任务内容">快捷任务内容</section>,
}));
vi.mock('../features/logs/LogSearchPage', () => ({
  LogSearchPage: ({ searchIntent, onSearchIntentConsumed }: {
    searchIntent?: { serverId: string; path: string; keyword: string } | null;
    onSearchIntentConsumed?: () => void;
  }) => <section aria-label="日志检索内容">
    日志检索内容
    {searchIntent && <span>{`${searchIntent.serverId}|${searchIntent.path}|${searchIntent.keyword}`}</span>}
    {searchIntent && <button type="button" onClick={onSearchIntentConsumed}>消费搜索意图</button>}
  </section>,
}));
vi.mock('../features/transfers/FileTransferPage', () => ({
  FileTransferPage: ({ onSearchRemoteFile }: {
    onSearchRemoteFile?: (intent: { serverId: string; path: string; keyword: string }) => void;
  }) => <section aria-label="文件传输内容">
    文件传输内容
    <button type="button" onClick={() => onSearchRemoteFile?.({ serverId: 'server-1', path: '/srv/report.log', keyword: 'report.log' })}>模拟搜索远程文件</button>
  </section>,
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
  const bootstrap = {
    state: 'ready' as const,
    dataRoot: 'D:\\Codex Project\\轻量化SSH快捷工具\\data',
    dataRootSource: 'registry' as const,
    dataRootMutable: true,
    lastDataMigration: null,
  };
  const scrollTo = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.acknowledgeDataRootMigration.mockResolvedValue(undefined);
    apiMocks.openDataRootFolder.mockResolvedValue(undefined);
    scrollTo.mockClear();
    Object.defineProperty(HTMLElement.prototype, 'scrollTo', {
      configurable: true,
      value: scrollTo,
    });
  });

  it('explains a completed migration, keeps the old directory available and acknowledges it', async () => {
    const user = userEvent.setup();
    render(<AppShell bootstrap={{
      ...bootstrap,
      dataRoot: 'D:\\new-data',
      lastDataMigration: migration('completed'),
    }} />);

    expect(screen.getByText('数据目录迁移完成')).toBeVisible();
    expect(screen.getByText(/旧目录仍完整保留在 D:\\old-data/)).toBeVisible();
    await user.click(screen.getByRole('button', { name: '打开旧目录' }));
    expect(apiMocks.openDataRootFolder).toHaveBeenCalledWith('last_source');
    await user.click(screen.getByRole('button', { name: '关闭迁移结果' }));
    expect(apiMocks.acknowledgeDataRootMigration).toHaveBeenCalledWith('migration-1');
    expect(screen.queryByText('数据目录迁移完成')).not.toBeInTheDocument();
  });

  it('takes a failed migration directly to settings for a safe retry', async () => {
    const user = userEvent.setup();
    render(<AppShell bootstrap={{ ...bootstrap, lastDataMigration: migration('failed') }} />);

    expect(screen.getByText('数据目录迁移失败，原目录仍在使用')).toBeVisible();
    await user.click(screen.getByRole('button', { name: '前往设置重试' }));
    expect(screen.getByLabelText('设置内容')).toBeVisible();
  });

  it('offers the complete navigation and opens the workflow builder', async () => {
    const user = userEvent.setup();
    render(<AppShell bootstrap={bootstrap} />);

    expect(screen.getByText('本地安全保护已开启')).toBeVisible();
    expect(screen.queryByText(/WebView/i)).not.toBeInTheDocument();

    for (const name of ['首页', '服务器', '快捷任务', '日志检索', '文件传输', '工作流', '执行记录', '下载文件', '设置']) {
      expect(screen.getByRole('button', { name })).toBeVisible();
    }

    await user.click(screen.getByRole('button', { name: '快捷任务' }));
    expect(screen.getByLabelText('快捷任务内容')).toBeVisible();
    expect(scrollTo).toHaveBeenLastCalledWith({ top: 0, left: 0 });

    await user.click(screen.getByRole('button', { name: '日志检索' }));
    expect(screen.getByLabelText('日志检索内容')).toBeVisible();

    await user.click(screen.getByRole('button', { name: '工作流' }));
    expect(screen.getByLabelText('工作流内容')).toBeVisible();
    expect(screen.queryByText('工作流将在下一里程碑开放')).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: '设置' }));
    expect(screen.getByLabelText('设置内容')).toBeVisible();
  });

  it('carries a one-shot remote-file search intent from SFTP to log search', async () => {
    const user = userEvent.setup();
    render(<AppShell bootstrap={bootstrap} />);

    await user.click(screen.getByRole('button', { name: '文件传输' }));
    await user.click(screen.getByRole('button', { name: '模拟搜索远程文件' }));

    expect(screen.getByLabelText('日志检索内容')).toBeVisible();
    expect(screen.getByText('server-1|/srv/report.log|report.log')).toBeVisible();
    await user.click(screen.getByRole('button', { name: '消费搜索意图' }));
    await user.click(screen.getByRole('button', { name: '文件传输' }));
    await user.click(screen.getByRole('button', { name: '日志检索' }));
    expect(screen.queryByText('server-1|/srv/report.log|report.log')).not.toBeInTheDocument();
  });
});

function migration(phase: 'completed' | 'failed') {
  return {
    schemaVersion: 1,
    migrationId: 'migration-1',
    source: 'D:\\old-data',
    target: 'D:\\new-data',
    sourceMode: 'registry' as const,
    parentPid: 42,
    fileCount: 12,
    totalBytes: 1024,
    copiedFiles: phase === 'completed' ? 12 : 5,
    copiedBytes: phase === 'completed' ? 1024 : 512,
    phase,
    errorSummary: phase === 'failed' ? '完整性校验失败' : null,
    startedAt: 1,
    updatedAt: 2,
    acknowledged: false,
  };
}
