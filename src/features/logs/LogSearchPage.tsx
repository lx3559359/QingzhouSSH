import {
  ArrowLeft,
  ArrowRight,
  DownloadSimple,
  FileMagnifyingGlass,
  MagnifyingGlass,
  SpinnerGap,
} from '@phosphor-icons/react';
import { useEffect, useState } from 'react';
import type { FormEvent } from 'react';

import type {
  ExecutionDetails,
  ExecutionEvent,
  LogMatch,
  LogSearchRequest,
  ServerProfile,
} from '../../api/contracts';
import { api } from '../../api/tauri';
import { LogResultsTable } from './LogResultsTable';

const PAGE_SIZE = 50;

function classifiedError(category: string | null | undefined, fallback?: string | null) {
  switch (category) {
    case 'permission':
      return '远端账号无权读取该日志，请检查文件权限或更换账号。';
    case 'compatibility':
      return '远端缺少日志检索所需的 grep、awk 或 gzip 命令。';
    case 'output_limit_exceeded':
      return '检索输出超过安全上限，请缩小时间范围或降低结果上限。';
    case 'validation':
      return fallback || '检索条件无效，请检查日志路径、时间范围和结果上限。';
    case 'ssh':
    case 'ssh_command':
      return '远端检索未成功，请确认服务器在线且日志路径存在。';
    default:
      return fallback || '日志检索失败，请检查服务器连接后重试。';
  }
}

function errorPayload(error: unknown): { code?: string; message?: string } {
  if (typeof error === 'object' && error !== null) {
    const value = error as Record<string, unknown>;
    return {
      code: typeof value.code === 'string' ? value.code : undefined,
      message: typeof value.message === 'string' ? value.message : undefined,
    };
  }
  return {};
}

export function LogSearchPage() {
  const [servers, setServers] = useState<ServerProfile[]>([]);
  const [serverId, setServerId] = useState('');
  const [path, setPath] = useState('/var/log/syslog');
  const [keyword, setKeyword] = useState('');
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [contextLines, setContextLines] = useState('2');
  const [limit, setLimit] = useState('500');
  const [startTime, setStartTime] = useState('');
  const [endTime, setEndTime] = useState('');
  const [isSearching, setIsSearching] = useState(false);
  const [details, setDetails] = useState<ExecutionDetails | null>(null);
  const [items, setItems] = useState<LogMatch[]>([]);
  const [searched, setSearched] = useState(false);
  const [matchCount, setMatchCount] = useState<number | null>(null);
  const [cursor, setCursor] = useState<string | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [cursorHistory, setCursorHistory] = useState<(string | null)[]>([]);
  const [pageNumber, setPageNumber] = useState(1);
  const [message, setMessage] = useState<{ kind: 'error' | 'success'; text: string } | null>(null);
  const [downloadName, setDownloadName] = useState('log-results.txt');

  useEffect(() => {
    let active = true;
    api.listServers()
      .then((loaded) => {
        if (!active) return;
        setServers(loaded);
        setServerId((current) => current || loaded[0]?.id || '');
      })
      .catch(() => active && setMessage({ kind: 'error', text: '服务器列表加载失败，请稍后重试。' }));
    return () => { active = false; };
  }, []);

  const loadPage = async (executionId: string, targetCursor: string | null, targetPage: number) => {
    const page = await api.readLogResultPage(executionId, targetCursor, PAGE_SIZE);
    setItems(page.items);
    setCursor(targetCursor);
    setNextCursor(page.nextCursor);
    setPageNumber(targetPage);
    setSearched(true);
  };

  const onEvent = (event: ExecutionEvent) => {
    if (event.type === 'finished' && event.result && typeof event.result === 'object') {
      const count = (event.result as Record<string, unknown>).count;
      if (typeof count === 'number') setMatchCount(count);
    }
    if (event.type === 'failed') {
      setMessage({ kind: 'error', text: classifiedError(event.category, event.message) });
    }
  };

  const submitSearch = async (event: FormEvent) => {
    event.preventDefault();
    if (!serverId) {
      setMessage({ kind: 'error', text: '请先添加并选择一台服务器。' });
      return;
    }
    setIsSearching(true);
    setMessage(null);
    setDetails(null);
    setItems([]);
    setSearched(false);
    setMatchCount(null);
    setCursor(null);
    setNextCursor(null);
    setCursorHistory([]);
    setPageNumber(1);
    const request: LogSearchRequest = {
      path,
      keyword,
      caseSensitive,
      contextLines: Number(contextLines),
      limit: Number(limit),
      startTime: startTime || null,
      endTime: endTime || null,
    };
    try {
      const result = await api.searchLogs(serverId, request, onEvent);
      setDetails(result);
      if (result.record.status !== 'succeeded') {
        setMessage({
          kind: 'error',
          text: classifiedError(result.record.errorCategory, result.record.errorMessage),
        });
        return;
      }
      if (matchCount === null && result.record.outputSummary) {
        const count = Number.parseInt(result.record.outputSummary, 10);
        if (Number.isFinite(count)) setMatchCount(count);
      }
      await loadPage(result.record.id, null, 1);
    } catch (error) {
      const payload = errorPayload(error);
      setMessage({ kind: 'error', text: classifiedError(payload.code, payload.message) });
    } finally {
      setIsSearching(false);
    }
  };

  const nextPage = async () => {
    if (!details || !nextCursor) return;
    setMessage(null);
    try {
      setCursorHistory((history) => [...history, cursor]);
      await loadPage(details.record.id, nextCursor, pageNumber + 1);
    } catch (error) {
      const payload = errorPayload(error);
      setMessage({ kind: 'error', text: classifiedError(payload.code, payload.message) });
    }
  };

  const previousPage = async () => {
    if (!details || cursorHistory.length === 0) return;
    const targetCursor = cursorHistory[cursorHistory.length - 1];
    setMessage(null);
    try {
      await loadPage(details.record.id, targetCursor, pageNumber - 1);
      setCursorHistory((history) => history.slice(0, -1));
    } catch (error) {
      const payload = errorPayload(error);
      setMessage({ kind: 'error', text: classifiedError(payload.code, payload.message) });
    }
  };

  const download = async () => {
    if (!details || details.record.status !== 'succeeded') return;
    setMessage(null);
    try {
      const relativePath = await api.downloadLogResult(details.record.id, downloadName);
      setMessage({ kind: 'success', text: `已保存到 ${relativePath}` });
    } catch (error) {
      const payload = errorPayload(error);
      setMessage({ kind: 'error', text: classifiedError(payload.code, payload.message) });
    }
  };

  return (
    <section className="log-search-page" aria-labelledby="log-search-title">
      <header className="page-heading">
        <div>
          <span className="eyebrow">远端检索 · 本地分页 · 结果可下载</span>
          <h1 id="log-search-title">日志检索</h1>
          <p>支持普通日志与 .gz 压缩日志，结果文件统一保存在项目数据目录内。</p>
        </div>
      </header>

      <div className="log-search-layout">
        <form className="silver-card log-search-form" onSubmit={submitSearch}>
          <header><FileMagnifyingGlass weight="duotone" /><div><h2>检索条件</h2><small>路径和关键词会在 Rust 层再次校验</small></div></header>
          <label><span>目标服务器</span><select aria-label="目标服务器" value={serverId} onChange={(event) => setServerId(event.target.value)} required><option value="">请选择服务器</option>{servers.map((server) => <option key={server.id} value={server.id}>{server.name}</option>)}</select></label>
          <label><span>日志路径</span><input aria-label="日志路径" value={path} onChange={(event) => setPath(event.target.value)} placeholder="/var/log/app.log" required /></label>
          <label><span>关键词</span><input aria-label="关键词" value={keyword} onChange={(event) => setKeyword(event.target.value)} maxLength={512} required /></label>
          <div className="log-form-grid">
            <label><span>开始时间</span><input aria-label="开始时间" type="datetime-local" value={startTime} onChange={(event) => setStartTime(event.target.value)} /></label>
            <label><span>结束时间</span><input aria-label="结束时间" type="datetime-local" value={endTime} onChange={(event) => setEndTime(event.target.value)} /></label>
            <label><span>上下文行数</span><input aria-label="上下文行数" type="number" min="0" max="20" value={contextLines} onChange={(event) => setContextLines(event.target.value)} required /></label>
            <label><span>结果上限</span><input aria-label="结果上限" type="number" min="1" max="10000" value={limit} onChange={(event) => setLimit(event.target.value)} required /></label>
          </div>
          <label className="checkbox-field"><input aria-label="区分大小写" type="checkbox" checked={caseSensitive} onChange={(event) => setCaseSensitive(event.target.checked)} /><span>区分大小写</span></label>
          <button className="primary-button" type="submit" disabled={isSearching || servers.length === 0}>{isSearching ? <SpinnerGap className="spin" weight="bold" /> : <MagnifyingGlass weight="bold" />}{isSearching ? '正在检索' : '开始检索'}</button>
        </form>

        <article className="silver-card log-results-panel">
          <header className="log-results-header">
            <div><span className="eyebrow">分页预览</span><h2>检索结果</h2><small>{matchCount === null ? '尚未检索' : `共匹配 ${matchCount} 条`}</small></div>
            <div className="log-download-controls"><input aria-label="下载文件名" value={downloadName} onChange={(event) => setDownloadName(event.target.value)} /><button className="secondary-button" type="button" onClick={download} disabled={!details || details.record.status !== 'succeeded'}><DownloadSimple weight="bold" />下载结果</button></div>
          </header>
          {message && <p className={`inline-message inline-message--${message.kind}`} role={message.kind === 'error' ? 'alert' : 'status'}>{message.text}</p>}
          {isSearching && <div className="log-search-progress" role="status"><SpinnerGap className="spin" weight="bold" /><span>正在远端扫描日志并生成脱敏结果…</span></div>}
          <LogResultsTable items={items} searched={searched} />
          <footer className="log-pagination"><span>第 {pageNumber} 页 · 每页 {PAGE_SIZE} 条</span><div><button className="secondary-button" type="button" onClick={previousPage} disabled={cursorHistory.length === 0}><ArrowLeft weight="bold" />上一页</button><button className="secondary-button" type="button" onClick={nextPage} disabled={!nextCursor}>下一页<ArrowRight weight="bold" /></button></div></footer>
        </article>
      </div>
    </section>
  );
}
