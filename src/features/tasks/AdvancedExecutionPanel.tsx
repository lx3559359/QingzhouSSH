import { Code, FileCode, ShieldWarning } from '@phosphor-icons/react';
import { useMemo, useState } from 'react';

import type { CustomExecutionRequest, ExecutionDetails, ExecutionEvent, ServerProfile } from '../../api/contracts';
import { api } from '../../api/tauri';
import { ExecutionDrawer } from './ExecutionDrawer';

interface AdvancedExecutionPanelProps {
  servers: ServerProfile[];
  serverId: string;
}

export function AdvancedExecutionPanel({ servers, serverId }: AdvancedExecutionPanelProps) {
  const [mode, setMode] = useState<'command' | 'script'>('command');
  const [content, setContent] = useState('');
  const [timeout, setTimeoutValue] = useState('60');
  const [confirming, setConfirming] = useState(false);
  const [running, setRunning] = useState(false);
  const [events, setEvents] = useState<ExecutionEvent[]>([]);
  const [details, setDetails] = useState<ExecutionDetails | null>(null);
  const [error, setError] = useState('');
  const server = servers.find((item) => item.id === serverId);
  const summary = useMemo(() => `${content.length === 0 ? 0 : content.split(/\r?\n/).length} 行 · ${content.length} 字符`, [content]);

  const requestConfirmation = () => {
    if (!serverId || !content.trim()) {
      setError(!serverId ? '请先选择目标服务器。' : '请输入要执行的命令或脚本。');
      return;
    }
    setError('');
    setConfirming(true);
  };

  const execute = async () => {
    setConfirming(false);
    setRunning(true);
    setEvents([]);
    setDetails(null);
    setError('');
    const request: CustomExecutionRequest = {
      mode,
      content,
      timeoutSeconds: Number(timeout),
      dangerousConfirmed: true,
    };
    try {
      const result = await api.startCustomExecution(serverId, request, (event) => {
        setEvents((current) => [...current, event].slice(-500));
      });
      setDetails(result);
    } catch {
      setError('高级执行失败，请检查内容、服务器连接和超时设置。');
    } finally {
      setRunning(false);
    }
  };

  const cancel = async () => {
    const started = events.slice().reverse().find((event) => event.type === 'started');
    if (started?.type === 'started') await api.cancelExecution(started.executionId);
  };

  return (
    <div className="advanced-execution-grid">
      <article className="silver-card advanced-execution-panel">
        <header><div><span className="eyebrow">受控高级模式</span><h2>命令与脚本</h2></div><ShieldWarning className="danger-icon" weight="fill" /></header>
        <p className="security-caption"><ShieldWarning weight="duotone" />不提供交互式终端、PTY 或持续 stdin；每次执行都是有超时和输出上限的一次性任务。</p>
        <div className="segmented-control" aria-label="高级执行类型">
          <label className={mode === 'command' ? 'is-selected' : ''}><input type="radio" checked={mode === 'command'} onChange={() => setMode('command')} /><Code weight="bold" />单条命令</label>
          <label className={mode === 'script' ? 'is-selected' : ''}><input type="radio" checked={mode === 'script'} onChange={() => setMode('script')} /><FileCode weight="bold" />多行脚本</label>
        </div>
        <label className="advanced-content-field"><span>{mode === 'command' ? '命令内容' : '脚本内容'}</span><textarea aria-label={mode === 'command' ? '命令内容' : '脚本内容'} value={content} onChange={(event) => setContent(event.target.value)} spellCheck={false} placeholder={mode === 'command' ? '例如：uptime' : '#!/bin/sh\nset -eu'} /></label>
        <label className="advanced-timeout-field"><span>超时秒数</span><input aria-label="超时秒数" type="number" min="1" max="3600" value={timeout} onChange={(event) => setTimeoutValue(event.target.value)} required /></label>
        <p className="advanced-summary">将执行：{mode === 'command' ? '单条命令' : '多行脚本'} · {summary} · 超时 {timeout || '—'} 秒</p>
        {error && <p className="inline-message inline-message--error" role="alert">{error}</p>}
        <button className="danger-button" type="button" disabled={running || !serverId} onClick={requestConfirmation}>检查并运行</button>
      </article>
      <ExecutionDrawer events={events} details={details} running={running} onCancel={() => void cancel()} />

      {confirming && server && (
        <div className="dialog-backdrop" role="presentation">
          <section className="silver-card modal-card danger-confirm" role="dialog" aria-modal="true" aria-labelledby="advanced-confirm-title">
            <ShieldWarning className="danger-confirm__icon" weight="fill" />
            <div><span className="eyebrow">不展示完整内容</span><h2 id="advanced-confirm-title">确认高级执行</h2></div>
            <dl><div><dt>目标服务器</dt><dd>{server.name}</dd></div><div><dt>执行类型</dt><dd>{mode === 'command' ? '单条命令' : '多行脚本'}</dd></div><div><dt>内容摘要</dt><dd>{summary}</dd></div><div><dt>超时限制</dt><dd>{timeout} 秒</dd></div></dl>
            <p className="security-caption"><ShieldWarning weight="duotone" />该内容不会保存到浏览器本地存储，执行历史只保留脱敏摘要。</p>
            <div className="modal-actions"><button className="secondary-button" type="button" onClick={() => setConfirming(false)}>取消</button><button className="danger-button" type="button" onClick={() => void execute()}>确认并运行</button></div>
          </section>
        </div>
      )}
    </div>
  );
}
