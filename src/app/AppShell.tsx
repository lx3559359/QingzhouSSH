import {
  ClockCounterClockwise,
  DownloadSimple,
  FlowArrow,
  GearSix,
  HardDrives,
  House,
  Lightning,
  FileMagnifyingGlass,
  UploadSimple,
} from '@phosphor-icons/react';
import { lazy, Suspense, useEffect, useRef, useState } from 'react';

import type { RemoteFileSearchIntent } from '../features/transfers/FileTransferPage';
import { DataMigrationResultNotice } from '../features/settings/DataMigrationResultNotice';
import type { ReadyBootstrapStatus } from '../api/contracts';

const ServerListPage = lazy(() => import('../features/servers/ServerListPage').then((module) => ({ default: module.ServerListPage })));
const TaskPage = lazy(() => import('../features/tasks/TaskPage').then((module) => ({ default: module.TaskPage })));
const LogSearchPage = lazy(() => import('../features/logs/LogSearchPage').then((module) => ({ default: module.LogSearchPage })));
const FileTransferPage = lazy(() => import('../features/transfers/FileTransferPage').then((module) => ({ default: module.FileTransferPage })));
const DownloadsPage = lazy(() => import('../features/downloads/DownloadsPage').then((module) => ({ default: module.DownloadsPage })));
const ExecutionHistoryPage = lazy(() => import('../features/history/ExecutionHistoryPage').then((module) => ({ default: module.ExecutionHistoryPage })));
const WorkflowPage = lazy(() => import('../features/workflows/WorkflowPage').then((module) => ({ default: module.WorkflowPage })));
const SettingsPage = lazy(() => import('../features/settings/SettingsPage').then((module) => ({ default: module.SettingsPage })));

type PageKey =
  | 'home'
  | 'servers'
  | 'tasks'
  | 'logs'
  | 'transfers'
  | 'workflows'
  | 'history'
  | 'downloads'
  | 'settings';

const navigation = [
  { key: 'home' as const, label: '首页', icon: House },
  { key: 'servers' as const, label: '服务器', icon: HardDrives },
  { key: 'tasks' as const, label: '快捷任务', icon: Lightning },
  { key: 'logs' as const, label: '日志检索', icon: FileMagnifyingGlass },
  { key: 'transfers' as const, label: '文件传输', icon: UploadSimple },
  { key: 'workflows' as const, label: '工作流', icon: FlowArrow },
  { key: 'history' as const, label: '执行记录', icon: ClockCounterClockwise },
  { key: 'downloads' as const, label: '下载文件', icon: DownloadSimple },
  { key: 'settings' as const, label: '设置', icon: GearSix },
];

export function AppShell({ bootstrap }: { bootstrap: ReadyBootstrapStatus }) {
  const [page, setPage] = useState<PageKey>('home');
  const [searchIntent, setSearchIntent] = useState<RemoteFileSearchIntent | null>(null);
  const contentRef = useRef<HTMLElement>(null);

  useEffect(() => {
    contentRef.current?.scrollTo?.({ top: 0, left: 0 });
  }, [page]);

  return (
    <div className="workspace-shell">
      <nav className="silver-card side-navigation" aria-label="主导航">
        <div className="side-navigation__title">
          <span>QZ</span>
          <div>
            <strong>操作中心</strong>
            <small>无终端快捷工具</small>
          </div>
        </div>
        <div className="side-navigation__items">
          {navigation.map(({ key, label, icon: Icon }) => (
            <button
              type="button"
              key={key}
              className={page === key ? 'is-active' : ''}
              aria-current={page === key ? 'page' : undefined}
              onClick={() => setPage(key)}
            >
              <Icon weight={page === key ? 'fill' : 'duotone'} aria-hidden="true" />
              {label}
            </button>
          ))}
        </div>
        <div className="side-navigation__security">
          <span className="status-dot" aria-hidden="true" />
          <strong>本地安全保护已开启</strong>
        </div>
      </nav>

      <section className="workspace-content" ref={contentRef}>
        <DataMigrationResultNotice journal={bootstrap.lastDataMigration} onRetry={() => setPage('settings')} />
        <Suspense fallback={<PageLoadingState />}>
          {page === 'home' && <HomePage onNavigate={setPage} />}
          {page === 'servers' && <ServerListPage />}
          {page === 'tasks' && <TaskPage />}
          {page === 'logs' && <LogSearchPage searchIntent={searchIntent} onSearchIntentConsumed={() => setSearchIntent(null)} />}
          {page === 'transfers' && <FileTransferPage onSearchRemoteFile={(intent) => { setSearchIntent(intent); setPage('logs'); }} />}
          {page === 'workflows' && <WorkflowPage />}
          {page === 'history' && <ExecutionHistoryPage />}
          {page === 'downloads' && <DownloadsPage />}
          {page === 'settings' && <SettingsPage bootstrap={bootstrap} />}
        </Suspense>
      </section>
    </div>
  );
}

function PageLoadingState() {
  return (
    <section className="page-loading-state" role="status" aria-live="polite">
      <div className="silver-card page-loading-state__card">
        <strong>正在打开功能…</strong>
        <span>首次打开会加载对应模块</span>
      </div>
    </section>
  );
}

function HomePage({ onNavigate }: { onNavigate: (page: PageKey) => void }) {
  return (
    <section className="dashboard-page" aria-labelledby="dashboard-title">
      <header className="page-heading">
        <div>
          <span className="eyebrow">QingzhouSSH · 本地操作台</span>
          <h1 id="dashboard-title">今天要处理什么？</h1>
          <p>选择服务器后，通过受控任务完成系统检查、服务管理、日志和文件操作。</p>
        </div>
      </header>
      <div className="dashboard-grid">
        <button className="silver-card dashboard-action" type="button" onClick={() => onNavigate('servers')}>
          <span className="feature-icon feature-icon--purple"><HardDrives weight="duotone" /></span>
          <span><strong>管理服务器</strong><small>核验主机身份并自动识别系统</small></span>
        </button>
        <button className="silver-card dashboard-action" type="button" onClick={() => onNavigate('tasks')}>
          <span className="feature-icon feature-icon--orange"><Lightning weight="duotone" /></span>
          <span><strong>运行快捷任务</strong><small>参数化执行，不开放交互式终端</small></span>
        </button>
      </div>
      <article className="silver-card dashboard-guide">
        <span className="eyebrow">推荐流程</span>
        <ol>
          <li><strong>连接</strong><span>添加服务器并确认 SSH 指纹</span></li>
          <li><strong>选择</strong><span>自动匹配当前 Linux 系统的任务实现</span></li>
          <li><strong>执行</strong><span>查看实时输出、结果文件和脱敏历史</span></li>
        </ol>
      </article>
    </section>
  );
}
