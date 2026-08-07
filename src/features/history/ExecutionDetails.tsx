import { CheckCircle, Clock, File, PlayCircle } from '@phosphor-icons/react';

import type { ExecutionDetails as Details } from '../../api/contracts';
import { executionStatusLabel } from '../../presentation/executionLabels';

function time(value: number | null) {
  return value == null ? '—' : new Date(value).toLocaleString('zh-CN');
}

export function ExecutionDetails({ details }: { details: Details }) {
  const { record } = details;
  return (
    <aside className="silver-card history-details" aria-labelledby="execution-details-title">
      <header><div><span className="eyebrow">脱敏记录</span><h2 id="execution-details-title">执行详情</h2></div><span className={`history-status history-status--${record.status}`}>{executionStatusLabel(record.status)}</span></header>
      <div className="history-detail-summary"><strong>{record.taskId}</strong><span>退出码 {record.exitCode ?? '—'}</span><span>耗时 {record.durationMs ?? '—'} ms</span></div>
      <ol className="execution-timeline"><li><Clock weight="duotone" /><span><strong>已创建</strong><small>{time(record.createdAt)}</small></span></li><li><PlayCircle weight="duotone" /><span><strong>已开始</strong><small>{time(record.startedAt)}</small></span></li><li><CheckCircle weight="duotone" /><span><strong>已完成</strong><small>{time(record.finishedAt)}</small></span></li></ol>
      <section><h3>参数摘要</h3>{details.parameters.length ? <dl className="detail-parameters">{details.parameters.map((parameter) => <div key={parameter.name}><dt>{parameter.name}</dt><dd>{parameter.displayValue}</dd></div>)}</dl> : <p>无参数</p>}</section>
      <section><h3>脱敏日志摘要</h3><pre className="history-output">{record.outputSummary || record.errorMessage || '无输出摘要'}</pre></section>
      <section><h3>关联文件</h3>{details.files.length ? <ul className="detail-files">{details.files.map((file) => <li key={file.id}><File weight="duotone" /><span><strong>{file.relativePath}</strong><small>{file.purpose} · {file.sizeBytes} B</small></span></li>)}</ul> : <p>无关联文件</p>}</section>
    </aside>
  );
}
