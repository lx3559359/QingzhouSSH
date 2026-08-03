import { ClockCounterClockwise, SpinnerGap } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import type { ExecutionDetails as Details, ExecutionFilter, ExecutionRecord, ExecutionStatus, ServerProfile } from '../../api/contracts';
import { api } from '../../api/tauri';
import { ExecutionDetails } from './ExecutionDetails';

function dateStart(value: string) { return value ? new Date(`${value}T00:00:00`).getTime() : undefined; }
function dateEnd(value: string) { return value ? new Date(`${value}T23:59:59.999`).getTime() : undefined; }

export function ExecutionHistoryPage() {
  const [servers, setServers] = useState<ServerProfile[]>([]);
  const [serverId, setServerId] = useState('');
  const [category, setCategory] = useState('');
  const [status, setStatus] = useState('');
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [records, setRecords] = useState<ExecutionRecord[]>([]);
  const [selected, setSelected] = useState<Details | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => { api.listServers().then(setServers).catch(() => setError('服务器列表加载失败')); }, []);
  useEffect(() => {
    let active = true;
    const filter: ExecutionFilter = {};
    if (serverId) filter.serverId = serverId;
    if (category) filter.category = category;
    if (status) filter.status = status as ExecutionStatus;
    const createdFrom = dateStart(from);
    const createdTo = dateEnd(to);
    if (createdFrom !== undefined) filter.createdFrom = createdFrom;
    if (createdTo !== undefined) filter.createdTo = createdTo;
    setLoading(true);
    api.listExecutions(filter).then((loaded) => { if (active) setRecords(loaded); }).catch(() => active && setError('执行记录加载失败')).finally(() => active && setLoading(false));
    return () => { active = false; };
  }, [serverId, category, status, from, to]);

  const openDetails = async (record: ExecutionRecord) => {
    const details = await api.getExecution(record.id);
    if (details) setSelected(details);
  };

  return (
    <section className="history-page" aria-labelledby="history-title">
      <header className="page-heading"><div><span className="eyebrow">可审计 · 已脱敏 · 可筛选</span><h1 id="history-title">执行记录</h1><p>中断时仍为 running 的记录会被恢复为 uncertain，避免误报成功。</p></div></header>
      <div className="silver-card history-filters"><label><span>服务器</span><select aria-label="历史服务器" value={serverId} onChange={(event) => setServerId(event.target.value)}><option value="">全部服务器</option>{servers.map((server) => <option key={server.id} value={server.id}>{server.name}</option>)}</select></label><label><span>类别</span><select aria-label="执行类别" value={category} onChange={(event) => setCategory(event.target.value)}><option value="">全部类别</option>{['system', 'service', 'logs', 'transfer', 'advanced'].map((item) => <option key={item}>{item}</option>)}</select></label><label><span>状态</span><select aria-label="执行状态" value={status} onChange={(event) => setStatus(event.target.value)}><option value="">全部状态</option>{['queued', 'running', 'succeeded', 'failed', 'cancelled', 'uncertain'].map((item) => <option key={item}>{item}</option>)}</select></label><label><span>开始日期</span><input aria-label="开始日期" type="date" value={from} onChange={(event) => setFrom(event.target.value)} /></label><label><span>结束日期</span><input aria-label="结束日期" type="date" value={to} onChange={(event) => setTo(event.target.value)} /></label></div>
      {error && <p className="inline-message inline-message--error" role="alert">{error}</p>}
      <div className="history-layout"><article className="silver-card history-list">{loading ? <div className="page-loading"><SpinnerGap className="spin" weight="bold" />正在加载…</div> : records.length === 0 ? <div className="log-empty-state"><ClockCounterClockwise weight="duotone" /><strong>没有符合条件的记录</strong></div> : records.map((record) => <button type="button" className={selected?.record.id === record.id ? 'history-row is-selected' : 'history-row'} key={record.id} aria-label={`查看 ${record.taskId}`} onClick={() => openDetails(record)}><span><strong>{record.taskId}</strong><small>{new Date(record.createdAt).toLocaleString('zh-CN')}</small></span><span>{servers.find((server) => server.id === record.serverId)?.name || record.serverId}</span><span className={`history-status history-status--${record.status}`}>{record.status}</span></button>)}</article>{selected ? <ExecutionDetails details={selected} /> : <article className="silver-card history-details history-details--empty"><ClockCounterClockwise weight="duotone" /><strong>选择一条记录查看详情</strong><span>参数、时间线、脱敏摘要和关联文件会显示在这里。</span></article>}</div>
    </section>
  );
}
