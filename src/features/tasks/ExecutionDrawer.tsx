import { CheckCircle, SpinnerGap, StopCircle, XCircle } from '@phosphor-icons/react';

import type { ExecutionDetails, ExecutionEvent } from '../../api/contracts';

interface ExecutionDrawerProps {
  events: ExecutionEvent[];
  details: ExecutionDetails | null;
  running: boolean;
  onCancel: () => void;
}

export function ExecutionDrawer({ events, details, running, onCancel }: ExecutionDrawerProps) {
  const output = events
    .filter((event): event is Extract<ExecutionEvent, { type: 'stdout' | 'stderr' }> =>
      event.type === 'stdout' || event.type === 'stderr',
    )
    .map((event) => event.text)
    .join('');
  const failed = events.slice().reverse().find((event) => event.type === 'failed');

  return (
    <aside className="silver-card execution-drawer" aria-label="运行结果">
      <header>
        <div>
          <span className="eyebrow">实时执行</span>
          <h2>{running ? '任务运行中' : details ? '任务已结束' : '等待运行'}</h2>
        </div>
        {running ? <SpinnerGap className="spin" weight="bold" /> : details?.record.status === 'succeeded' ? <CheckCircle weight="fill" /> : failed ? <XCircle weight="fill" /> : null}
      </header>
      <div className="execution-meta">
        <span>状态 <strong>{details?.record.status ?? (running ? 'running' : 'idle')}</strong></span>
        <span>退出码 <strong>{details?.record.exitCode ?? '—'}</strong></span>
        <span>耗时 <strong>{details?.record.durationMs != null ? `${details.record.durationMs} ms` : '—'}</strong></span>
      </div>
      <pre className="execution-output" aria-label="命令输出">{output || '运行后将在这里显示脱敏输出。'}</pre>
      {failed?.type === 'failed' && <p className="inline-message inline-message--error">{failed.message}</p>}
      {running && (
        <button className="danger-button" type="button" onClick={onCancel}>
          <StopCircle weight="bold" />取消执行
        </button>
      )}
    </aside>
  );
}
