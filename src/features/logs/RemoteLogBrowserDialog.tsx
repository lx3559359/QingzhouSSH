import {
  ArrowUp,
  FileText,
  Folder,
  FolderOpen,
  SpinnerGap,
  X,
} from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import type { DirectoryListing } from '../../api/contracts';
import { api, asAppError } from '../../api/tauri';
import { directorySessionCache } from '../file-browser/directorySessionCache';

interface RemoteLogBrowserDialogProps {
  serverId: string;
  initialPath: string;
  onClose: () => void;
  onSelect: (path: string) => void;
}

export function RemoteLogBrowserDialog({
  serverId,
  initialPath,
  onClose,
  onSelect,
}: RemoteLogBrowserDialogProps) {
  const [listing, setListing] = useState<DirectoryListing | null>(null);
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState('');

  const load = async (path: string, force = false) => {
    const cached = directorySessionCache.peekRemote(serverId, path);
    const preserved = cached ?? (listing?.path === path ? listing : null);
    if (preserved) setListing(preserved);
    setLoading(!preserved);
    setRefreshing(Boolean(force && preserved));
    setError('');
    try {
      const loader = () => api.listRemoteDirectory(serverId, path);
      const next = force
        ? await directorySessionCache.refreshRemote(serverId, path, loader)
        : await directorySessionCache.loadRemote(serverId, path, loader);
      setListing(next);
      directorySessionCache.rememberRemotePath(serverId, next.path);
    } catch (cause) {
      const payload = asAppError(cause);
      setError(`无法读取该远程目录，请确认账号有访问权限。${payload.message ? ` 技术详情：${payload.message}` : ''}${preserved ? ' 当前显示上次读取结果。' : ''}`);
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  };

  useEffect(() => {
    void load(initialPath);
  }, [initialPath, serverId]);

  return (
    <div className="dialog-backdrop" role="presentation">
      <section className="silver-card modal-card remote-log-browser" role="dialog" aria-modal="true" aria-labelledby="remote-log-browser-title">
        <header className="modal-header">
          <div>
            <span className="eyebrow">SFTP 远程浏览</span>
            <h2 id="remote-log-browser-title">选择远程日志</h2>
            <p>打开文件夹查找日志，选择后会自动填入完整路径。</p>
          </div>
          <button className="icon-button" type="button" aria-label="关闭远程日志选择" onClick={onClose}><X weight="bold" /></button>
        </header>

        <div className="browser-pathbar">
          <button className="secondary-button" type="button" disabled={!listing?.parent || loading || refreshing} onClick={() => listing?.parent && void load(listing.parent)}><ArrowUp weight="bold" />上级</button>
          <code title={listing?.path}>{listing?.path ?? initialPath}</code>
          {refreshing && <span className="browser-refreshing" role="status"><SpinnerGap className="spin" />正在刷新</span>}
          <button className="secondary-button" type="button" disabled={loading || refreshing} onClick={() => void load(listing?.path ?? initialPath, true)}>刷新</button>
        </div>

        {error && <p className="inline-message inline-message--error" role="alert">{error}</p>}
        {loading ? (
          <div className="browser-loading" role="status"><SpinnerGap className="spin" />正在读取远程目录…</div>
        ) : listing?.entries.length ? (
          <div className="browser-entry-list">
            {listing.entries.map((entry) => entry.kind === 'directory' ? (
              <button type="button" key={entry.path} aria-label={`打开目录 ${entry.name}`} onClick={() => void load(entry.path)}>
                <FolderOpen weight="duotone" /><span><strong>{entry.name}</strong><small>文件夹</small></span>
              </button>
            ) : entry.kind === 'file' ? (
              <button type="button" key={entry.path} aria-label={`选择日志 ${entry.name}`} onClick={() => onSelect(entry.path)}>
                <FileText weight="duotone" /><span><strong>{entry.name}</strong><small>{entry.size == null ? '日志文件' : `${entry.size.toLocaleString()} B`}</small></span>
              </button>
            ) : (
              <div className="browser-entry-disabled" key={entry.path}><Folder weight="duotone" /><span><strong>{entry.name}</strong><small>暂不支持此文件类型</small></span></div>
            ))}
          </div>
        ) : (
          <div className="browser-empty"><FolderOpen weight="duotone" /><strong>此目录为空</strong><span>请返回上级目录继续查找。</span></div>
        )}
      </section>
    </div>
  );
}
