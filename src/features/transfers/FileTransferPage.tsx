import {
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  CheckCircle,
  File,
  Folder,
  FolderOpen,
  SpinnerGap,
  StopCircle,
} from '@phosphor-icons/react';
import { open } from '@tauri-apps/plugin-dialog';
import { useEffect, useRef, useState } from 'react';
import type { MouseEvent as ReactMouseEvent } from 'react';

import type {
  BrowserEntry,
  DirectoryListing,
  ExecutionDetails,
  ExecutionEvent,
  ServerProfile,
} from '../../api/contracts';
import { api, asAppError } from '../../api/tauri';
import { ContextMenu } from '../../components/ContextMenu';
import type { ContextMenuItem } from '../../components/ContextMenu';
import { copyText } from '../../lib/clipboard';
import { directorySessionCache } from '../file-browser/directorySessionCache';

function formatBytes(bytes: number) {
  if (bytes >= 1024 * 1024) return `${Number((bytes / (1024 * 1024)).toFixed(1))} MB`;
  if (bytes >= 1024) return `${Number((bytes / 1024).toFixed(1))} KB`;
  return `${bytes} B`;
}

function transferMessage(details: ExecutionDetails) {
  if (details.record.status === 'cancelled') return '传输已取消';
  if (details.record.status === 'uncertain') return '远端传输状态无法确认，请核对目标文件。';
  if (details.record.status === 'failed') {
    switch (details.record.errorCategory) {
      case 'permission': return '远程账号没有读写目标目录的权限，请更换目录或账号。';
      case 'validation': return '文件选择无效，请重新选择来源文件和目标目录。';
      case 'ssh': return '服务器连接中断，请确认网络和 SSH 服务后重试。';
      default: return details.record.errorMessage || 'SFTP 传输失败，请检查服务器连接、目录权限和磁盘空间。';
    }
  }
  return '传输成功';
}

function remoteFilePath(directory: string, name: string) {
  return directory === '/' ? `/${name}` : `${directory}/${name}`;
}

interface DirectoryPaneProps {
  scope: 'local' | 'remote';
  title: string;
  listing: DirectoryListing | null;
  selected: BrowserEntry | null;
  loading: boolean;
  refreshing: boolean;
  error: string;
  onOpenDirectory: (path: string) => void;
  onSelectFile: (entry: BrowserEntry) => void;
  onRefresh: () => void;
  onChooseDirectory?: () => void;
  onEntryContextMenu: (entry: BrowserEntry, event: ReactMouseEvent<HTMLButtonElement>) => void;
}

function DirectoryPane({
  scope,
  title,
  listing,
  selected,
  loading,
  refreshing,
  error,
  onOpenDirectory,
  onSelectFile,
  onRefresh,
  onChooseDirectory,
  onEntryContextMenu,
}: DirectoryPaneProps) {
  const scopeLabel = scope === 'local' ? '本地' : '远程';
  const busy = loading || refreshing;
  return (
    <section className={`silver-card sftp-pane sftp-pane--${scope}`} role="region" aria-label={`${scopeLabel}文件浏览器`}>
      <header>
        <span className={`feature-icon ${scope === 'local' ? 'feature-icon--blue' : 'feature-icon--green'}`}><FolderOpen weight="duotone" /></span>
        <div><span className="eyebrow">{scope === 'local' ? '本地文件' : 'SFTP 服务器'}</span><h2>{title}</h2></div>
      </header>
      <div className="sftp-pathbar">
        <button className="icon-button" type="button" aria-label={`${scopeLabel}返回上级`} disabled={!listing?.parent || busy} onClick={() => listing?.parent && onOpenDirectory(listing.parent)}><ArrowUp weight="bold" /></button>
        <code title={listing?.path}>{listing?.path ?? '正在读取…'}</code>
        {refreshing && <span className="browser-refreshing" role="status"><SpinnerGap className="spin" />正在刷新</span>}
        <button className="secondary-button" type="button" disabled={busy} onClick={onRefresh}>刷新</button>
        {onChooseDirectory && <button className="secondary-button" type="button" disabled={busy} onClick={onChooseDirectory}>选择目录</button>}
      </div>
      {error && <p className="inline-message inline-message--error" role="alert">{error}</p>}
      <div className="sftp-entry-head"><span>名称</span><span>大小</span></div>
      <div className="sftp-entry-list">
        {loading ? (
          <div className="browser-loading" role="status"><SpinnerGap className="spin" />正在读取{scopeLabel}目录…</div>
        ) : listing?.entries.length ? listing.entries.map((entry) => {
          const directory = entry.kind === 'directory';
          const supported = directory || entry.kind === 'file';
          const label = directory
            ? `打开${scopeLabel}目录 ${entry.name}`
            : `选择${scopeLabel}文件 ${entry.name}`;
          return (
            <button
              type="button"
              key={entry.path}
              className={selected?.path === entry.path ? 'is-selected' : ''}
              aria-label={label}
              disabled={!supported}
              onClick={() => directory ? onOpenDirectory(entry.path) : onSelectFile(entry)}
              onContextMenu={(event) => onEntryContextMenu(entry, event)}
            >
              {directory ? <Folder weight="duotone" /> : <File weight="duotone" />}
              <span title={entry.name}>{entry.name}</span>
              <small>{entry.size == null ? (directory ? '文件夹' : '—') : formatBytes(entry.size)}</small>
            </button>
          );
        }) : (
          <div className="browser-empty"><FolderOpen weight="duotone" /><strong>此目录为空</strong></div>
        )}
      </div>
      <footer>{selected ? <span>已选择：<strong>{selected.name}</strong></span> : <span>请选择一个文件</span>}</footer>
    </section>
  );
}

export interface RemoteFileSearchIntent {
  serverId: string;
  path: string;
  keyword: string;
}

interface FileTransferPageProps {
  onSearchRemoteFile?: (intent: RemoteFileSearchIntent) => void;
}

type BrowserContext = {
  position: { x: number; y: number };
  scope: 'local' | 'remote';
  entry: BrowserEntry;
};

export function FileTransferPage({ onSearchRemoteFile }: FileTransferPageProps = {}) {
  const [servers, setServers] = useState<ServerProfile[]>([]);
  const [serverId, setServerId] = useState('');
  const [localListing, setLocalListing] = useState<DirectoryListing | null>(null);
  const [remoteListing, setRemoteListing] = useState<DirectoryListing | null>(null);
  const [selectedLocal, setSelectedLocal] = useState<BrowserEntry | null>(null);
  const [selectedRemote, setSelectedRemote] = useState<BrowserEntry | null>(null);
  const [localLoading, setLocalLoading] = useState(true);
  const [remoteLoading, setRemoteLoading] = useState(false);
  const [localRefreshing, setLocalRefreshing] = useState(false);
  const [remoteRefreshing, setRemoteRefreshing] = useState(false);
  const [localError, setLocalError] = useState('');
  const [remoteError, setRemoteError] = useState('');
  const [overwrite, setOverwrite] = useState(false);
  const [running, setRunning] = useState(false);
  const [executionId, setExecutionId] = useState<string | null>(null);
  const [transferred, setTransferred] = useState(0);
  const [total, setTotal] = useState<number | null>(null);
  const [percent, setPercent] = useState<number | null>(null);
  const [speed, setSpeed] = useState(0);
  const [sha256, setSha256] = useState<string | null>(null);
  const [location, setLocation] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [source, setSource] = useState('');
  const [target, setTarget] = useState('');
  const [browserContext, setBrowserContext] = useState<BrowserContext | null>(null);
  const startedAt = useRef<number | null>(null);

  const loadLocal = async (path: string | null, force = false) => {
    const cached = directorySessionCache.peekLocal(path);
    const preserved = cached ?? (localListing?.path === path ? localListing : null);
    if (preserved) setLocalListing(preserved);
    setLocalLoading(!preserved);
    setLocalRefreshing(Boolean(force && preserved));
    setLocalError('');
    try {
      const loader = () => api.listLocalDirectory(path);
      const listing = force
        ? await directorySessionCache.refreshLocal(path, loader)
        : await directorySessionCache.loadLocal(path, loader);
      setLocalListing(listing);
      setSelectedLocal(null);
    } catch (cause) {
      setLocalError(`无法读取本地目录：${asAppError(cause).message}${preserved ? '。当前显示上次读取结果' : ''}`);
    } finally {
      setLocalLoading(false);
      setLocalRefreshing(false);
    }
  };

  const loadRemote = async (path: string, force = false) => {
    if (!serverId) return;
    const cached = directorySessionCache.peekRemote(serverId, path);
    const preserved = cached ?? (remoteListing?.path === path ? remoteListing : null);
    if (preserved) setRemoteListing(preserved);
    setRemoteLoading(!preserved);
    setRemoteRefreshing(Boolean(force && preserved));
    setRemoteError('');
    try {
      const loader = () => api.listRemoteDirectory(serverId, path);
      const listing = force
        ? await directorySessionCache.refreshRemote(serverId, path, loader)
        : await directorySessionCache.loadRemote(serverId, path, loader);
      setRemoteListing(listing);
      directorySessionCache.rememberRemotePath(serverId, listing.path);
      setSelectedRemote(null);
    } catch (cause) {
      setRemoteError(`无法读取远程目录，请检查连接和权限。技术详情：${asAppError(cause).message}${preserved ? '。当前显示上次读取结果' : ''}`);
    } finally {
      setRemoteLoading(false);
      setRemoteRefreshing(false);
    }
  };

  useEffect(() => {
    void loadLocal(null);
    api.listServers().then((loaded) => {
      setServers(loaded);
      setServerId(loaded[0]?.id || '');
    }).catch(() => setStatus('服务器列表加载失败'));
  }, []);

  useEffect(() => {
    if (serverId) void loadRemote(directorySessionCache.lastRemotePath(serverId));
    else setRemoteListing(null);
  }, [serverId]);

  const resetProgress = () => {
    setTransferred(0);
    setTotal(null);
    setPercent(null);
    setSpeed(0);
    setSha256(null);
    setLocation(null);
    setStatus(null);
    setExecutionId(null);
    startedAt.current = null;
  };

  const onEvent = (event: ExecutionEvent) => {
    if (event.type === 'started') {
      setExecutionId(event.executionId);
      startedAt.current = event.emittedAt;
    }
    if (event.type === 'progress') {
      setTransferred(event.transferred);
      setTotal(event.total);
      setPercent(event.percent);
      const elapsedSeconds = startedAt.current == null ? 0 : (event.emittedAt - startedAt.current) / 1000;
      if (elapsedSeconds > 0) setSpeed(event.transferred / elapsedSeconds);
    }
    if (event.type === 'fileProduced') setLocation(event.file.relativePath);
    if (event.type === 'finished' && event.result && typeof event.result === 'object') {
      const result = event.result as Record<string, unknown>;
      if (typeof result.sha256 === 'string') setSha256(result.sha256);
      if (typeof result.location === 'string') setLocation(result.location);
    }
    if (event.type === 'failed') setStatus(event.category === 'cancelled' ? '传输已取消' : event.message);
  };

  const applyResult = (details: ExecutionDetails) => {
    setExecutionId(details.record.id);
    setStatus(transferMessage(details));
    if (details.record.status !== 'succeeded') {
      setSha256(null);
      return;
    }
    const file = details.files.find((item) => item.purpose === 'download');
    if (file) {
      setLocation(file.relativePath);
      setSha256(file.sha256);
      setTransferred(file.sizeBytes);
      setTotal(file.sizeBytes);
      setPercent(100);
    } else if (details.record.outputSummary) {
      const hash = details.record.outputSummary.match(/[a-f\d]{64}/i)?.[0];
      if (hash) setSha256(hash);
    }
  };

  const upload = async (entry: BrowserEntry | null = selectedLocal) => {
    if (!serverId || !entry || entry.kind !== 'file' || !remoteListing || running) return;
    const destination = remoteFilePath(remoteListing.path, entry.name);
    resetProgress();
    setSelectedLocal(entry);
    setSource(entry.path);
    setTarget(destination);
    setRunning(true);
    try {
      applyResult(await api.uploadFile(serverId, { localPath: entry.path, remotePath: destination, overwrite }, onEvent));
      await loadRemote(remoteListing.path, true);
    } catch (cause) {
      setStatus(`SFTP 上传失败：${asAppError(cause).message}`);
    } finally {
      setRunning(false);
    }
  };

  const download = async (entry: BrowserEntry | null = selectedRemote) => {
    if (!serverId || !entry || entry.kind !== 'file' || running) return;
    resetProgress();
    setSelectedRemote(entry);
    setSource(entry.path);
    setTarget(`项目数据目录/downloads/${entry.name}`);
    setRunning(true);
    try {
      applyResult(await api.downloadFile(serverId, { remotePath: entry.path, suggestedName: entry.name, overwrite }, onEvent));
      if (localListing) await loadLocal(localListing.path, true);
    } catch (cause) {
      setStatus(`SFTP 下载失败：${asAppError(cause).message}`);
    } finally {
      setRunning(false);
    }
  };

  const chooseLocalDirectory = async () => {
    const selected = await open({ directory: true, multiple: false, title: '选择本地目录' });
    if (typeof selected === 'string') await loadLocal(selected);
  };

  const cancel = async () => {
    if (!executionId) return;
    await api.cancelExecution(executionId);
    setStatus('正在取消传输…');
  };

  const copyBrowserValue = async (value: string, description: string) => {
    try {
      await copyText(value);
      setStatus(`${description}已复制`);
    } catch (cause) {
      setStatus(asAppError(cause).message);
    }
  };

  const openBrowserContext = (
    scope: 'local' | 'remote',
    entry: BrowserEntry,
    event: ReactMouseEvent<HTMLButtonElement>,
  ) => {
    event.preventDefault();
    if (entry.kind === 'file') {
      if (scope === 'local') setSelectedLocal(entry);
      else setSelectedRemote(entry);
    }
    setBrowserContext({ position: { x: event.clientX, y: event.clientY }, scope, entry });
  };

  const contextItems = (context: BrowserContext): ContextMenuItem[] => {
    const { entry, scope } = context;
    if (entry.kind === 'directory') {
      return [
        {
          id: 'open',
          label: '打开文件夹',
          onSelect: () => scope === 'local' ? loadLocal(entry.path) : loadRemote(entry.path),
        },
        {
          id: 'refresh',
          label: '刷新当前目录',
          onSelect: () => scope === 'local'
            ? loadLocal(localListing?.path ?? null, true)
            : loadRemote(remoteListing?.path ?? '/', true),
        },
        {
          id: 'copy-path',
          label: '复制完整路径',
          onSelect: () => copyBrowserValue(entry.path, '完整路径'),
        },
      ];
    }
    if (scope === 'local') {
      return [
        {
          id: 'upload',
          label: '上传',
          disabled: !remoteListing || running,
          disabledReason: running ? '当前有传输正在进行' : '请先读取远程目录',
          onSelect: () => upload(entry),
        },
        { id: 'copy-name', label: '复制文件名', onSelect: () => copyBrowserValue(entry.name, '文件名') },
        { id: 'copy-path', label: '复制完整路径', onSelect: () => copyBrowserValue(entry.path, '完整路径') },
      ];
    }
    return [
      {
        id: 'download',
        label: '下载',
        disabled: running,
        disabledReason: '当前有传输正在进行',
        onSelect: () => download(entry),
      },
      {
        id: 'search-content',
        label: '搜索文件内容',
        disabled: !onSearchRemoteFile,
        disabledReason: '当前页面无法打开日志检索',
        onSelect: () => onSearchRemoteFile?.({ serverId, path: entry.path, keyword: entry.name }),
      },
      { id: 'copy-name', label: '复制文件名', onSelect: () => copyBrowserValue(entry.name, '文件名') },
      { id: 'copy-path', label: '复制完整路径', onSelect: () => copyBrowserValue(entry.path, '完整路径') },
    ];
  };

  const showTransferDetails = Boolean(source || target || status || running);

  return (
    <section className="transfer-page" aria-labelledby="transfer-title">
      <header className="page-heading transfer-heading">
        <div><span className="eyebrow">双栏 SFTP · 分块传输 · SHA-256 校验</span><h1 id="transfer-title">文件传输</h1><p>像 SFTP 客户端一样浏览两端目录；下载始终写入 D 盘项目数据目录。</p></div>
        <label className="server-selector"><span>目标服务器</span><select aria-label="传输服务器" value={serverId} onChange={(event) => setServerId(event.target.value)}><option value="">请选择服务器</option>{servers.map((server) => <option key={server.id} value={server.id}>{server.name}</option>)}</select></label>
      </header>

      <div className="sftp-workspace">
        <DirectoryPane scope="local" title="本地目录" listing={localListing} selected={selectedLocal} loading={localLoading} refreshing={localRefreshing} error={localError} onOpenDirectory={(path) => void loadLocal(path)} onSelectFile={setSelectedLocal} onRefresh={() => void loadLocal(localListing?.path ?? null, true)} onChooseDirectory={() => void chooseLocalDirectory()} onEntryContextMenu={(entry, event) => openBrowserContext('local', entry, event)} />
        <aside className="sftp-actions" aria-label="传输操作">
          <button className="primary-button" type="button" aria-label="上传到右侧目录" title="把左侧选中的本地文件上传到右侧当前目录" disabled={!selectedLocal || !remoteListing || running} onClick={() => void upload()}><ArrowRight weight="bold" /><span>上传</span></button>
          <button className="success-button" type="button" aria-label="下载到项目目录" title="把右侧选中的远程文件下载到项目数据目录" disabled={!selectedRemote || running} onClick={() => void download()}><ArrowLeft weight="bold" /><span>下载</span></button>
          <label className="checkbox-field"><input aria-label="允许覆盖" type="checkbox" checked={overwrite} onChange={(event) => setOverwrite(event.target.checked)} /><span className="sftp-overwrite-copy"><span>允许覆盖同名文件</span><small>覆盖</small></span></label>
          <small>下载位置固定为项目数据目录下的 downloads，避免占用 C 盘。</small>
        </aside>
        <DirectoryPane scope="remote" title="远程目录" listing={remoteListing} selected={selectedRemote} loading={remoteLoading} refreshing={remoteRefreshing} error={remoteError} onOpenDirectory={(path) => void loadRemote(path)} onSelectFile={setSelectedRemote} onRefresh={() => void loadRemote(remoteListing?.path ?? '/', true)} onEntryContextMenu={(entry, event) => openBrowserContext('remote', entry, event)} />
      </div>

      {showTransferDetails && <article
        className="silver-card transfer-status-card sftp-status-card"
        role="region"
        aria-label="传输状态"
      >
        <header><div><span className="eyebrow">实时状态</span><h2>{status ?? '等待选择文件'}</h2></div>{sha256 ? <CheckCircle weight="fill" /> : running ? <SpinnerGap className="spin" weight="bold" /> : null}</header>
        <dl className="transfer-paths"><div><dt>来源</dt><dd>{source || '—'}</dd></div><div><dt>目标</dt><dd>{target || '—'}</dd></div></dl>
        <div className="transfer-progress"><div><span style={{ width: `${percent ?? 0}%` }} /></div><strong>{percent == null ? '—' : `${Number(percent.toFixed(1))}%`}</strong></div>
        <div className="transfer-metrics"><span><small>已传输</small><strong>{formatBytes(transferred)}{total == null ? '' : ` / ${formatBytes(total)}`}</strong></span><span><small>平均速度</small><strong>{speed > 0 ? `${formatBytes(speed)}/s` : '—'}</strong></span><span><small>完整性</small><strong>{sha256 ? 'SHA-256 已校验' : '等待校验'}</strong></span></div>
        {sha256 && <code className="transfer-hash">{sha256}</code>}
        {location && <p className="inline-message inline-message--success">{location}</p>}
        {running && executionId && <button className="danger-button" type="button" onClick={() => void cancel()}><StopCircle weight="bold" />取消传输</button>}
      </article>}
      {browserContext && (
        <ContextMenu
          position={browserContext.position}
          items={contextItems(browserContext)}
          onClose={() => setBrowserContext(null)}
        />
      )}
    </section>
  );
}
