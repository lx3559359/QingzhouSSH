import { GearSix, Lightning, ShieldWarning, SpinnerGap, TerminalWindow } from '@phosphor-icons/react';
import { useEffect, useMemo, useState } from 'react';

import type {
  ExecutionDetails,
  ExecutionEvent,
  ServerProfile,
  TaskAvailability,
} from '../../api/contracts';
import { api } from '../../api/tauri';
import { describeTaskError } from '../../api/errors';
import type { UserFacingError } from '../../api/errors';
import { ExecutionDrawer } from './ExecutionDrawer';
import { ParameterForm } from './ParameterForm';
import { AdvancedExecutionPanel } from './AdvancedExecutionPanel';

export function TaskPage() {
  const [view, setView] = useState<'catalog' | 'advanced'>('catalog');
  const [servers, setServers] = useState<ServerProfile[]>([]);
  const [serverId, setServerId] = useState('');
  const [tasks, setTasks] = useState<TaskAvailability[]>([]);
  const [selectedId, setSelectedId] = useState('');
  const [parameters, setParameters] = useState<Record<string, unknown>>({});
  const [events, setEvents] = useState<ExecutionEvent[]>([]);
  const [details, setDetails] = useState<ExecutionDetails | null>(null);
  const [running, setRunning] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<UserFacingError | null>(null);

  useEffect(() => {
    api.listServers()
      .then((profiles) => {
        setServers(profiles);
        setServerId(profiles[0]?.id ?? '');
      })
      .catch((cause) => setError(describeTaskError(cause)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    if (!serverId) {
      setTasks([]);
      return;
    }
    setLoading(true);
    api.listTaskDefinitions(serverId)
      .then(setTasks)
      .catch((cause) => setError(describeTaskError(cause)))
      .finally(() => setLoading(false));
  }, [serverId]);

  const selected = useMemo(
    () => tasks.find((task) => task.definition.id === selectedId) ?? null,
    [selectedId, tasks],
  );
  const selectedServer = servers.find((server) => server.id === serverId) ?? null;

  function chooseTask(task: TaskAvailability) {
    setSelectedId(task.definition.id);
    setParameters(
      Object.fromEntries(
        task.definition.parameters
          .filter(
            (parameter) =>
              parameter.defaultValue !== null || parameter.kind.type === 'managedId',
          )
          .map((parameter) => [
            parameter.name,
            parameter.kind.type === 'managedId'
              ? crypto.randomUUID()
              : parameter.defaultValue,
          ]),
      ),
    );
    setEvents([]);
    setDetails(null);
    setError(null);
  }

  async function execute(dangerousConfirmed: boolean) {
    if (!selected || !serverId) return;
    setConfirming(false);
    setRunning(true);
    setEvents([]);
    setDetails(null);
    setError(null);
    try {
      const result = await api.startTaskExecution(
        serverId,
        {
          taskId: selected.definition.id,
          parameters,
          dangerousConfirmed,
        },
        (event) => setEvents((current) => [...current, event].slice(-500)),
      );
      setDetails(result);
    } catch (cause) {
      setError(describeTaskError(cause));
    } finally {
      setRunning(false);
    }
  }

  function requestRun() {
    if (!selected) return;
    if (selected.definition.riskLevel === 'dangerous') setConfirming(true);
    else void execute(false);
  }

  async function cancel() {
    const started = events.find((event) => event.type === 'started');
    if (started?.type === 'started') await api.cancelExecution(started.executionId);
  }

  return (
    <section className="task-page" aria-labelledby="tasks-title">
      <header className="page-heading task-page__heading">
        <div>
          <span className="eyebrow">自动匹配系统 · 参数化执行</span>
          <h1 id="tasks-title">快捷任务</h1>
          <p>无需 SSH 终端，Rust 会重新校验参数并选择兼容命令。</p>
        </div>
        <label className="server-selector">
          <span>目标服务器</span>
          <select aria-label="目标服务器" value={serverId} onChange={(event) => setServerId(event.target.value)}>
            {servers.length === 0 && <option value="">请先添加服务器</option>}
            {servers.map((server) => <option key={server.id} value={server.id}>{server.name}</option>)}
          </select>
        </label>
      </header>

      <div className="task-view-switch" aria-label="任务模式">
        <button type="button" className={view === 'catalog' ? 'is-active' : ''} onClick={() => setView('catalog')}>内置任务</button>
        <button type="button" className={view === 'advanced' ? 'is-active' : ''} onClick={() => setView('advanced')}>高级执行</button>
      </div>

      {error && (
        <div className="inline-message inline-message--error page-alert task-error" role="alert">
          <strong>{error.summary}</strong>
          {error.retryable && <span>你可以修正后直接再次运行，任务不会自动执行其他命令。</span>}
          {error.detail && (
            <details>
              <summary>查看技术详情</summary>
              <code>{error.detail}</code>
            </details>
          )}
        </div>
      )}
      {view === 'advanced' ? (
        <AdvancedExecutionPanel servers={servers} serverId={serverId} />
      ) : loading ? (
        <div className="silver-card loading-card" role="status"><SpinnerGap className="spin" />正在匹配任务…</div>
      ) : servers.length === 0 ? (
        <article className="silver-card milestone-notice"><Lightning weight="duotone" /><div><h2>请先添加服务器</h2><p>快捷任务需要目标服务器和已信任的 SSH 主机身份。</p></div></article>
      ) : (
        <div className="task-workspace">
          <section className="task-library" aria-label="任务库">
            <div className="task-card-grid">
              {tasks.map((task) => (
                <button
                  className={`silver-card task-card ${selectedId === task.definition.id ? 'is-selected' : ''}`}
                  type="button"
                  key={task.definition.id}
                  aria-label={`选择任务 ${task.definition.title}`}
                  disabled={!task.compatible}
                  onClick={() => chooseTask(task)}
                >
                  <span className={`feature-icon ${task.definition.category === 'service' ? 'feature-icon--orange' : task.definition.category === 'logs' ? 'feature-icon--green' : 'feature-icon--blue'}`}>
                    {task.definition.category === 'service' ? <GearSix weight="duotone" /> : task.definition.category === 'logs' ? <TerminalWindow weight="duotone" /> : <Lightning weight="duotone" />}
                  </span>
                  <span className="task-card__copy">
                    <span className="task-card__meta">{task.definition.category} · v{task.definition.version}</span>
                    <h2>{task.definition.title}</h2>
                    <p>{task.definition.description}</p>
                    <span className={`risk-chip risk-chip--${task.definition.riskLevel}`}>{task.compatible ? task.definition.riskLevel : '不兼容'}</span>
                  </span>
                </button>
              ))}
            </div>
          </section>

          {selected && (
            <section className="task-detail-grid">
              <article className="silver-card task-parameters">
                <header><div><span className="eyebrow">任务参数</span><h2>{selected.definition.title}</h2></div>{selected.definition.riskLevel === 'dangerous' && <ShieldWarning className="danger-icon" weight="fill" />}</header>
                <ParameterForm definitions={selected.definition.parameters} values={parameters} onChange={(name, value) => setParameters((current) => ({ ...current, [name]: value }))} />
                <button className={selected.definition.riskLevel === 'dangerous' ? 'danger-button' : 'success-button'} type="button" disabled={running || !selected.compatible} onClick={requestRun}>运行任务</button>
              </article>
              <ExecutionDrawer events={events} details={details} running={running} onCancel={() => void cancel()} />
            </section>
          )}
        </div>
      )}

      {confirming && selected && selectedServer && (
        <div className="dialog-backdrop" role="presentation">
          <section className="silver-card modal-card danger-confirm" role="dialog" aria-modal="true" aria-labelledby="danger-confirm-title">
            <ShieldWarning className="danger-confirm__icon" weight="fill" />
            <div><span className="eyebrow">二次确认</span><h2 id="danger-confirm-title">确认危险操作</h2></div>
            <dl><div><dt>目标服务器</dt><dd>{selectedServer.name}</dd></div><div><dt>操作影响</dt><dd>{selected.definition.description}</dd></div></dl>
            <div className="modal-actions"><button className="secondary-button" type="button" onClick={() => setConfirming(false)}>取消</button><button className="danger-button" type="button" onClick={() => void execute(true)}>确认并运行</button></div>
          </section>
        </div>
      )}
    </section>
  );
}
