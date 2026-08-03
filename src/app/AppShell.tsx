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
import { useState } from 'react';

import { ServerListPage } from '../features/servers/ServerListPage';
import { TaskPage } from '../features/tasks/TaskPage';
import { LogSearchPage } from '../features/logs/LogSearchPage';
import { FileTransferPage } from '../features/transfers/FileTransferPage';
import { DownloadsPage } from '../features/downloads/DownloadsPage';
import { ExecutionHistoryPage } from '../features/history/ExecutionHistoryPage';

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

export function AppShell() {
  const [page, setPage] = useState<PageKey>('home');

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
          <span className="status-dot" />
          <div>
            <strong>本地安全模式</strong>
            <small>凭据不会进入 WebView</small>
          </div>
        </div>
      </nav>

      <section className="workspace-content">
        {page === 'home' && <HomePage onNavigate={setPage} />}
        {page === 'servers' && <ServerListPage />}
        {page === 'tasks' && <TaskPage />}
        {page === 'logs' && <LogSearchPage />}
        {page === 'transfers' && <FileTransferPage />}
        {page === 'workflows' && <WorkflowNotice />}
        {page === 'history' && <ExecutionHistoryPage />}
        {page === 'downloads' && <DownloadsPage />}
        {page === 'settings' && <SectionNotice title="设置" description="数据目录、安全策略与更新设置集中在这里。" />}
      </section>
    </div>
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

function WorkflowNotice() {
  return (
    <article className="silver-card milestone-notice">
      <span className="feature-icon feature-icon--blue"><FlowArrow weight="duotone" /></span>
      <div>
        <span className="eyebrow">Milestone 3</span>
        <h1>工作流将在下一里程碑开放</h1>
        <p>届时会复用当前已经验证的任务、日志和传输接口，加入条件分支、重试和恢复点。</p>
      </div>
    </article>
  );
}

function SectionNotice({ title, description }: { title: string; description: string }) {
  return (
    <article className="silver-card milestone-notice">
      <span className="feature-icon feature-icon--green"><GearSix weight="duotone" /></span>
      <div><h1>{title}</h1><p>{description}</p></div>
    </article>
  );
}
