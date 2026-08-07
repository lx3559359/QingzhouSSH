import {
  ArrowLeft,
  ArrowRight,
  DownloadSimple,
  FileMagnifyingGlass,
  FolderOpen,
  MagnifyingGlass,
  SpinnerGap,
} from '@phosphor-icons/react';
import { useEffect, useState } from 'react';
import type { FormEvent } from 'react';

import type {
  ExecutionDetails,
  ExecutionEvent,
  LogSearchTarget,
  LogSearchRequest,
  RemoteFileMatch,
  SearchResultItem,
  ServerProfile,
} from '../../api/contracts';
import { api } from '../../api/tauri';
import { ContextMenu } from '../../components/ContextMenu';
import type { ContextMenuItem } from '../../components/ContextMenu';
import { copyText } from '../../lib/clipboard';
import { LogResultsTable } from './LogResultsTable';
import { RemoteLogBrowserDialog } from './RemoteLogBrowserDialog';

const PAGE_SIZE = 50;

export interface LogSearchIntent {
  serverId: string;
  path: string;
  keyword: string;
}

interface LogSearchPageProps {
  searchIntent?: LogSearchIntent | null;
  onSearchIntentConsumed?: () => void;
}

function startingDirectory(path: string) {
  if (!path.startsWith('/')) return '/var/log';
  const lastSlash = path.lastIndexOf('/');
  return lastSlash <= 0 ? '/' : path.slice(0, lastSlash);
}

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

export function LogSearchPage({ searchIntent, onSearchIntentConsumed }: LogSearchPageProps = {}) {
  const [servers, setServers] = useState<ServerProfile[]>([]);
  const [serverId, setServerId] = useState('');
  const [target, setTarget] = useState<LogSearchTarget>('content');
  const [searchMode, setSearchMode] = useState<'smart' | 'path'>('smart');
  const [path, setPath] = useState('/var/log/syslog');
  const [keyword, setKeyword] = useState('');
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [contextLines, setContextLines] = useState('2');
  const [limit, setLimit] = useState('500');
  const [startTime, setStartTime] = useState('');
  const [endTime, setEndTime] = useState('');
  const [isSearching, setIsSearching] = useState(false);
  const [details, setDetails] = useState<ExecutionDetails | null>(null);
  const [items, setItems] = useState<SearchResultItem[]>([]);
  const [searched, setSearched] = useState(false);
  const [matchCount, setMatchCount] = useState<number | null>(null);
  const [cursor, setCursor] = useState<string | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [cursorHistory, setCursorHistory] = useState<(string | null)[]>([]);
  const [pageNumber, setPageNumber] = useState(1);
  const [message, setMessage] = useState<{ kind: 'error' | 'success'; text: string } | null>(null);
  const [downloadName, setDownloadName] = useState('log-results.txt');
  const [browserOpen, setBrowserOpen] = useState(false);
  const [resultContext, setResultContext] = useState<{
    position: { x: number; y: number };
    item: SearchResultItem;
  } | null>(null);

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

  useEffect(() => {
    if (!searchIntent) return;
    setTarget('content');
    setSearchMode('path');
    setServerId(searchIntent.serverId);
    setPath(searchIntent.path);
    setKeyword(searchIntent.keyword);
    setMessage({ kind: 'success', text: '已从文件传输带入远程文件，请输入或调整要查找的内容。' });
    onSearchIntentConsumed?.();
  }, [searchIntent, onSearchIntentConsumed]);

  const loadPage = async (executionId: string, targetCursor: string | null, targetPage: number) => {
    const page = await api.readLogResultPage(executionId, targetCursor, PAGE_SIZE);
    setItems(page.items);
    setCursor(targetCursor);
    setNextCursor(page.nextCursor);
    setPageNumber(targetPage);
    setSearched(true);
    return page;
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
    if (target === 'filename' && new TextEncoder().encode(keyword).length > 256) {
      setMessage({ kind: 'error', text: '文件名关键字不能超过 256 字节，请缩短后重试。' });
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
      target,
      path: target === 'filename' || searchMode === 'smart' ? '' : path,
      keyword,
      caseSensitive: target === 'content' ? caseSensitive : false,
      contextLines: target === 'content' ? Number(contextLines) : 0,
      limit: target === 'content' ? Number(limit) : 200,
      startTime: target === 'content' ? startTime || null : null,
      endTime: target === 'content' ? endTime || null : null,
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
      const summaryCount = result.record.outputSummary
        ? Number.parseInt(result.record.outputSummary, 10)
        : Number.NaN;
      const firstPage = await loadPage(result.record.id, null, 1);
      if (Number.isFinite(summaryCount)) {
        setMatchCount((current) => current ?? summaryCount);
      } else if (!firstPage.nextCursor) {
        setMatchCount((current) => current ?? firstPage.items.length);
      }
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

  const copyResultValue = async (value: string, description: string) => {
    try {
      await copyText(value);
      setMessage({ kind: 'success', text: `${description}已复制` });
    } catch (error) {
      const payload = errorPayload(error);
      setMessage({ kind: 'error', text: payload.message || '复制失败，请稍后重试。' });
    }
  };

  const downloadRemoteResult = async (item: RemoteFileMatch) => {
    if (!serverId) {
      setMessage({ kind: 'error', text: '请先选择服务器。' });
      return;
    }
    try {
      const result = await api.downloadFile(
        serverId,
        { remotePath: item.path, suggestedName: item.name, overwrite: false },
        () => undefined,
      );
      if (result.record.status !== 'succeeded') {
        setMessage({ kind: 'error', text: result.record.errorMessage || '文件下载失败，请检查权限和网络。' });
        return;
      }
      const produced = result.files.find((file) => file.purpose === 'download');
      setMessage({ kind: 'success', text: produced ? `已下载到 ${produced.relativePath}` : '文件已下载到项目数据目录。' });
    } catch (error) {
      const payload = errorPayload(error);
      setMessage({ kind: 'error', text: payload.message || '文件下载失败，请检查权限和网络。' });
    }
  };

  const searchFileContent = (item: RemoteFileMatch) => {
    setTarget('content');
    setSearchMode('path');
    setPath(item.path);
    setDetails(null);
    setItems([]);
    setSearched(false);
    setMatchCount(null);
    setCursor(null);
    setNextCursor(null);
    setCursorHistory([]);
    setPageNumber(1);
    setMessage({ kind: 'success', text: '已切换到该文件的内容检索，可保留或修改当前关键字。' });
  };

  const resultContextItems = (item: SearchResultItem): ContextMenuItem[] => {
    if (item.resultType === 'file') {
      return [
        { id: 'download', label: '下载文件', onSelect: () => downloadRemoteResult(item) },
        { id: 'search-content', label: '搜索文件内容', onSelect: () => searchFileContent(item) },
        { id: 'copy-path', label: '复制完整路径', onSelect: () => copyResultValue(item.path, '完整路径') },
      ];
    }
    return [
      {
        id: 'copy-line',
        label: '复制本行',
        onSelect: () => copyResultValue(`${item.path}:${item.lineNumber} ${item.text}`, '日志行'),
      },
      { id: 'copy-path', label: '复制日志路径', onSelect: () => copyResultValue(item.path, '日志路径') },
    ];
  };

  return (
    <section className="log-search-page" aria-labelledby="log-search-title">
      <header className="page-heading">
        <div>
          <span className="eyebrow">智能发现 · 远端检索 · 结果可下载</span>
          <h1 id="log-search-title">日志检索</h1>
          <p>只需输入想找的内容，客户端会自动定位常见日志；也可以切换到指定文件。</p>
        </div>
      </header>

      <div className="log-search-layout">
        <form className="silver-card log-search-form" onSubmit={submitSearch}>
          <header><FileMagnifyingGlass weight="duotone" /><div><h2>检索条件</h2><small>默认不要求了解 Linux 日志路径</small></div></header>
          <label><span>目标服务器</span><select aria-label="目标服务器" value={serverId} onChange={(event) => setServerId(event.target.value)} required><option value="">请选择服务器</option>{servers.map((server) => <option key={server.id} value={server.id}>{server.name}</option>)}</select></label>
          <fieldset className="log-search-modes log-search-targets">
            <legend>我要查什么</legend>
            <label className={target === 'content' ? 'is-active' : ''}><input aria-label="搜日志内容" type="radio" name="log-search-target" checked={target === 'content'} onChange={() => setTarget('content')} /><span><strong>搜日志内容</strong><small>在日志正文中查找错误、编号或 IP</small></span></label>
            <label className={target === 'filename' ? 'is-active' : ''}><input aria-label="找文件名" type="radio" name="log-search-target" checked={target === 'filename'} onChange={() => setTarget('filename')} /><span><strong>找文件名</strong><small>只记得文件名的一部分也能查找</small></span></label>
          </fieldset>
          {target === 'content' ? <>
            <fieldset className="log-search-modes">
              <legend>搜索方式</legend>
              <label className={searchMode === 'smart' ? 'is-active' : ''}><input aria-label="智能搜索（推荐）" type="radio" name="log-search-mode" checked={searchMode === 'smart'} onChange={() => setSearchMode('smart')} /><span><strong>智能搜索（推荐）</strong><small>自动寻找系统和常见应用日志</small></span></label>
              <label className={searchMode === 'path' ? 'is-active' : ''}><input aria-label="指定日志文件" type="radio" name="log-search-mode" checked={searchMode === 'path'} onChange={() => setSearchMode('path')} /><span><strong>指定日志文件</strong><small>知道路径时进行精确检索</small></span></label>
            </fieldset>
            {searchMode === 'smart' ? (
              <div className="smart-log-scope"><MagnifyingGlass weight="duotone" /><span><strong>不需要知道日志路径</strong><small>自动检查近期系统日志和常见应用日志，范围与文件数量均受安全限制。</small></span></div>
            ) : (
              <label className="log-path-field"><span>日志路径</span><span><input aria-label="日志路径" value={path} onChange={(event) => setPath(event.target.value)} placeholder="请选择远程日志" required /><button className="secondary-button" type="button" disabled={!serverId} onClick={() => setBrowserOpen(true)}><FolderOpen weight="duotone" />浏览服务器</button></span></label>
            )}
            <label><span>搜索内容</span><input aria-label="搜索内容" value={keyword} onChange={(event) => setKeyword(event.target.value)} maxLength={512} placeholder="例如：连接超时、登录失败、订单号或 IP 地址" required /></label>
            <div className="log-form-grid">
              <label><span>开始时间</span><input aria-label="开始时间" type="datetime-local" value={startTime} onChange={(event) => setStartTime(event.target.value)} /></label>
              <label><span>结束时间</span><input aria-label="结束时间" type="datetime-local" value={endTime} onChange={(event) => setEndTime(event.target.value)} /></label>
              <label><span>上下文行数</span><input aria-label="上下文行数" type="number" min="0" max="20" value={contextLines} onChange={(event) => setContextLines(event.target.value)} required /></label>
              <label><span>结果上限</span><input aria-label="结果上限" type="number" min="1" max="10000" value={limit} onChange={(event) => setLimit(event.target.value)} required /></label>
            </div>
            <label className="checkbox-field"><input aria-label="区分大小写" type="checkbox" checked={caseSensitive} onChange={(event) => setCaseSensitive(event.target.checked)} /><span>区分大小写</span></label>
          </> : <>
            <div className="smart-log-scope smart-file-scope"><MagnifyingGlass weight="duotone" /><span><strong>不需要知道文件路径</strong><small>安全查找 /var/log、/opt、/srv 和 /home，最多 6 层、200 个结果。</small></span></div>
            <label><span>文件名包含</span><input aria-label="文件名包含" value={keyword} onChange={(event) => setKeyword(event.target.value)} maxLength={256} placeholder="例如：requi、nginx、error.log" required /></label>
          </>}
          <button className="primary-button" type="submit" disabled={isSearching || servers.length === 0}>{isSearching ? <SpinnerGap className="spin" weight="bold" /> : <MagnifyingGlass weight="bold" />}{isSearching ? (target === 'filename' ? '正在查找' : '正在检索') : (target === 'filename' ? '开始查找' : '开始检索')}</button>
        </form>

        <article className="silver-card log-results-panel">
          <header className="log-results-header">
            <div><span className="eyebrow">分页预览</span><h2>检索结果</h2><small>{matchCount === null ? '尚未检索' : target === 'filename' ? `共找到 ${matchCount} 个文件` : `共匹配 ${matchCount} 条`}</small></div>
            <div className="log-download-controls"><input aria-label="下载文件名" value={downloadName} onChange={(event) => setDownloadName(event.target.value)} /><button className="secondary-button" type="button" onClick={download} disabled={!details || details.record.status !== 'succeeded'}><DownloadSimple weight="bold" />下载结果</button></div>
          </header>
          {message && <p className={`inline-message inline-message--${message.kind}`} role={message.kind === 'error' ? 'alert' : 'status'}>{message.text}</p>}
          {isSearching && <div className="log-search-progress" role="status"><SpinnerGap className="spin" weight="bold" /><span>{target === 'filename' ? '正在安全范围内查找文件名…' : searchMode === 'smart' ? '正在自动寻找日志并检索内容…' : '正在检索指定日志并生成脱敏结果…'}</span></div>}
          <LogResultsTable items={items} searched={searched} onItemContextMenu={(item, position) => setResultContext({ item, position })} />
          <footer className="log-pagination"><span>第 {pageNumber} 页 · 每页 {PAGE_SIZE} 条</span><div><button className="secondary-button" type="button" onClick={previousPage} disabled={cursorHistory.length === 0}><ArrowLeft weight="bold" />上一页</button><button className="secondary-button" type="button" onClick={nextPage} disabled={!nextCursor}>下一页<ArrowRight weight="bold" /></button></div></footer>
        </article>
      </div>
      {browserOpen && serverId && target === 'content' && searchMode === 'path' && (
        <RemoteLogBrowserDialog
          serverId={serverId}
          initialPath={startingDirectory(path)}
          onClose={() => setBrowserOpen(false)}
          onSelect={(selectedPath) => {
            setPath(selectedPath);
            setBrowserOpen(false);
          }}
        />
      )}
      {resultContext && (
        <ContextMenu
          position={resultContext.position}
          items={resultContextItems(resultContext.item)}
          onClose={() => setResultContext(null)}
        />
      )}
    </section>
  );
}
