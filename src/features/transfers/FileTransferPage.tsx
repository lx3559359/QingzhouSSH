import { ArrowDown, ArrowUp, CheckCircle, SpinnerGap, StopCircle } from '@phosphor-icons/react';
import { useEffect, useRef, useState } from 'react';
import type { FormEvent } from 'react';

import type { ExecutionDetails, ExecutionEvent, ServerProfile } from '../../api/contracts';
import { api } from '../../api/tauri';

type TransferMode = 'upload' | 'download';

function formatBytes(bytes: number) {
  if (bytes >= 1024 * 1024) return `${Number((bytes / (1024 * 1024)).toFixed(1))} MB`;
  if (bytes >= 1024) return `${Number((bytes / 1024).toFixed(1))} KB`;
  return `${bytes} B`;
}

function transferMessage(details: ExecutionDetails) {
  if (details.record.status === 'cancelled') return '传输已取消';
  if (details.record.status === 'uncertain') return '远端传输状态无法确认，请核对目标文件。';
  if (details.record.status === 'failed') return details.record.errorMessage || '传输失败，请检查路径、权限和磁盘空间。';
  return '传输成功';
}

export function FileTransferPage() {
  const [servers, setServers] = useState<ServerProfile[]>([]);
  const [serverId, setServerId] = useState('');
  const [mode, setMode] = useState<TransferMode>('upload');
  const [uploadLocalPath, setUploadLocalPath] = useState('');
  const [uploadRemotePath, setUploadRemotePath] = useState('');
  const [downloadRemotePath, setDownloadRemotePath] = useState('');
  const [downloadName, setDownloadName] = useState('');
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
  const startedAt = useRef<number | null>(null);

  useEffect(() => {
    api.listServers().then((loaded) => {
      setServers(loaded);
      setServerId(loaded[0]?.id || '');
    }).catch(() => setStatus('服务器列表加载失败'));
  }, []);

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

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!serverId) return;
    resetProgress();
    setRunning(true);
    try {
      const details = mode === 'upload'
        ? await api.uploadFile(serverId, { localPath: uploadLocalPath, remotePath: uploadRemotePath, overwrite }, onEvent)
        : await api.downloadFile(serverId, { remotePath: downloadRemotePath, suggestedName: downloadName, overwrite }, onEvent);
      setExecutionId(details.record.id);
      setStatus(transferMessage(details));
      if (details.record.status === 'succeeded') {
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
      } else {
        setSha256(null);
      }
    } catch {
      setStatus('传输失败，请检查路径、权限和磁盘空间。');
    } finally {
      setRunning(false);
    }
  };

  const cancel = async () => {
    if (!executionId) return;
    await api.cancelExecution(executionId);
    setStatus('正在取消传输…');
  };

  const source = mode === 'upload' ? uploadLocalPath : downloadRemotePath;
  const target = mode === 'upload' ? uploadRemotePath : location || `downloads/${downloadName}`;

  return (
    <section className="transfer-page" aria-labelledby="transfer-title">
      <header className="page-heading"><div><span className="eyebrow">SFTP · 分块传输 · SHA-256 校验</span><h1 id="transfer-title">文件传输</h1><p>下载文件只写入项目数据目录；中断文件保留为可识别的临时状态。</p></div></header>
      <div className="transfer-layout">
        <form className="silver-card transfer-form" onSubmit={submit}>
          <div className="segmented-control transfer-mode" aria-label="传输方向">
            <label className={mode === 'upload' ? 'is-selected' : ''}><input type="radio" checked={mode === 'upload'} onChange={() => setMode('upload')} /><ArrowUp weight="bold" />上传</label>
            <label className={mode === 'download' ? 'is-selected' : ''}><input type="radio" checked={mode === 'download'} onChange={() => setMode('download')} /><ArrowDown weight="bold" />下载</label>
          </div>
          <label><span>目标服务器</span><select aria-label="传输服务器" value={serverId} onChange={(event) => setServerId(event.target.value)} required><option value="">请选择服务器</option>{servers.map((server) => <option key={server.id} value={server.id}>{server.name}</option>)}</select></label>
          {mode === 'upload' ? <><label><span>本地文件绝对路径</span><input aria-label="上传本地路径" value={uploadLocalPath} onChange={(event) => setUploadLocalPath(event.target.value)} required /></label><label><span>远端文件绝对路径</span><input aria-label="上传远端路径" value={uploadRemotePath} onChange={(event) => setUploadRemotePath(event.target.value)} required /></label></> : <><label><span>远端文件绝对路径</span><input aria-label="下载远端路径" value={downloadRemotePath} onChange={(event) => setDownloadRemotePath(event.target.value)} required /></label><label><span>项目内本地文件名</span><input aria-label="本地文件名" value={downloadName} onChange={(event) => setDownloadName(event.target.value)} required /></label></>}
          <label className="checkbox-field"><input type="checkbox" checked={overwrite} onChange={(event) => setOverwrite(event.target.checked)} /><span>允许覆盖已存在的目标文件</span></label>
          <button className="primary-button" type="submit" disabled={running || !serverId}>{running ? <SpinnerGap className="spin" weight="bold" /> : mode === 'upload' ? <ArrowUp weight="bold" /> : <ArrowDown weight="bold" />}{running ? '传输中' : mode === 'upload' ? '开始上传' : '开始下载'}</button>
        </form>
        <article className="silver-card transfer-status-card">
          <header><div><span className="eyebrow">实时状态</span><h2>{status ?? '等待传输'}</h2></div>{sha256 ? <CheckCircle weight="fill" /> : running ? <SpinnerGap className="spin" weight="bold" /> : null}</header>
          <dl className="transfer-paths"><div><dt>来源</dt><dd>{source || '—'}</dd></div><div><dt>目标</dt><dd>{target || '—'}</dd></div></dl>
          <div className="transfer-progress"><div><span style={{ width: `${percent ?? 0}%` }} /></div><strong>{percent == null ? '—' : `${Number(percent.toFixed(1))}%`}</strong></div>
          <div className="transfer-metrics"><span><small>已传输</small><strong>{formatBytes(transferred)}{total == null ? '' : ` / ${formatBytes(total)}`}</strong></span><span><small>平均速度</small><strong>{speed > 0 ? `${formatBytes(speed)}/s` : '—'}</strong></span><span><small>完整性</small><strong>{sha256 ? 'SHA-256 已校验' : '等待校验'}</strong></span></div>
          {sha256 && <code className="transfer-hash">{sha256}</code>}
          {location && <p className="inline-message inline-message--success">{location}</p>}
          {running && executionId && <button className="danger-button" type="button" onClick={cancel}><StopCircle weight="bold" />取消传输</button>}
        </article>
      </div>
    </section>
  );
}
