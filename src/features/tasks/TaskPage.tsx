import { Lightning, SpinnerGap } from '@phosphor-icons/react';
import { useEffect, useMemo, useState } from 'react';

import type {
  ExecutionDetails,
  ExecutionEvent,
  OperationPreview,
  PersonalScriptDetails,
  PersonalScriptSummary,
  ServerProfile,
  TaskAvailability,
  TaskRemediationPreview,
} from '../../api/contracts';
import { describeTaskError } from '../../api/errors';
import type { UserFacingError } from '../../api/errors';
import { api } from '../../api/tauri';
import { AdvancedExecutionPanel } from './AdvancedExecutionPanel';
import { ScriptCenter } from './scripts/ScriptCenter';
import { ScriptRunDialog } from './scripts/ScriptRunDialog';
import { ToolCatalogList } from './library/ToolCatalogList';
import { ToolCategoryRail } from './library/ToolCategoryRail';
import { ToolDetailPane } from './library/ToolDetailPane';
import { ToolLibraryFilters } from './library/ToolLibraryFilters';
import { TaskRemediationDialog } from './library/TaskRemediationDialog';
import { buildToolLibrary, filterToolLibrary, groupCounts } from './library/toolLibrary';
import type { ToolLibraryItem, UnifiedToolCategory } from './library/types';

export function TaskPage() {
  const [view, setView] = useState<'library' | 'scripts' | 'advanced'>('library');
  const [servers, setServers] = useState<ServerProfile[]>([]);
  const [serverId, setServerId] = useState('');
  const [tasks, setTasks] = useState<TaskAvailability[]>([]);
  const [scripts, setScripts] = useState<PersonalScriptSummary[]>([]);
  const [selectedKey, setSelectedKey] = useState('');
  const [category, setCategory] = useState<UnifiedToolCategory | 'all'>('all');
  const [query, setQuery] = useState('');
  const [showUnavailable, setShowUnavailable] = useState(false);
  const [parameters, setParameters] = useState<Record<string, unknown>>({});
  const [events, setEvents] = useState<ExecutionEvent[]>([]);
  const [details, setDetails] = useState<ExecutionDetails | null>(null);
  const [running, setRunning] = useState(false);
  const [operationPreview, setOperationPreview] = useState<OperationPreview | null>(null);
  const [remediationPreview, setRemediationPreview] = useState<TaskRemediationPreview | null>(null);
  const [remediationBusy, setRemediationBusy] = useState(false);
  const [scriptRunDetails, setScriptRunDetails] = useState<PersonalScriptDetails | null>(null);
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
      setScripts([]);
      return;
    }
    setLoading(true);
    Promise.all([
      api.listTaskDefinitions(serverId),
      api.listPersonalScripts({ enabled: true }),
    ])
      .then(([nextTasks, nextScripts]) => {
        setTasks(nextTasks);
        setScripts(nextScripts);
      })
      .catch((cause) => setError(describeTaskError(cause)))
      .finally(() => setLoading(false));
  }, [serverId]);

  const libraryItems = useMemo(() => buildToolLibrary(tasks, scripts), [tasks, scripts]);
  const counts = useMemo(() => groupCounts(libraryItems), [libraryItems]);
  const visibleItems = useMemo(() => filterToolLibrary(libraryItems, {
    query,
    categories: category === 'all' ? undefined : [category],
    states: showUnavailable
      ? ['ready', 'remediable', 'permission_blocked', 'unsupported']
      : undefined,
  }), [category, libraryItems, query, showUnavailable]);
  const selected = useMemo(
    () => libraryItems.find((item) => `${item.source}:${item.id}` === selectedKey) ?? null,
    [libraryItems, selectedKey],
  );
  const selectedServer = servers.find((server) => server.id === serverId) ?? null;

  useEffect(() => {
    if (visibleItems.length === 0) {
      setSelectedKey('');
      return;
    }
    if (!visibleItems.some((item) => `${item.source}:${item.id}` === selectedKey)) {
      chooseItem(visibleItems[0]);
    }
    // Selection changes only when the active item leaves the visible collection.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visibleItems, selectedKey]);

  function chooseItem(item: ToolLibraryItem | undefined) {
    if (!item) return;
    setSelectedKey(`${item.source}:${item.id}`);
    setParameters(item.availability
      ? Object.fromEntries(
          item.availability.definition.parameters
            .filter((parameter) => parameter.defaultValue !== null || parameter.kind.type === 'managedId')
            .map((parameter) => [
              parameter.name,
              parameter.kind.type === 'managedId' ? crypto.randomUUID() : parameter.defaultValue,
            ]),
        )
      : {});
    setEvents([]);
    setDetails(null);
    setError(null);
  }

  async function executeSafeTask() {
    if (!selected?.availability || !serverId) return;
    setRunning(true);
    setEvents([]);
    setDetails(null);
    setError(null);
    try {
      const result = await api.startTaskExecution(
        serverId,
        {
          taskId: selected.availability.definition.id,
          parameters,
          dangerousConfirmed: false,
        },
        appendEvent,
      );
      setDetails(result);
    } catch (cause) {
      setError(describeTaskError(cause));
    } finally {
      setRunning(false);
    }
  }

  async function requestRun() {
    if (!selected || !serverId) return;
    if (selected.source === 'personal_script') {
      setRunning(true);
      setError(null);
      try {
        const script = await api.getPersonalScriptForEditor(selected.script.id);
        if (!script) throw new Error('脚本不存在或已被删除。');
        setScriptRunDetails(script);
      } catch (cause) {
        setError(describeTaskError(cause));
      } finally {
        setRunning(false);
      }
      return;
    }
    if (!selected.availability) return;
    if (selected.risk !== 'dangerous') {
      await executeSafeTask();
      return;
    }
    setRunning(true);
    setError(null);
    try {
      const preview = await api.previewOperation(serverId, {
        taskId: selected.availability.definition.id,
        taskVersion: selected.availability.definition.version,
        parameters,
      });
      setOperationPreview(preview);
    } catch (cause) {
      setError(describeTaskError(cause));
    } finally {
      setRunning(false);
    }
  }

  async function confirmDangerousOperation() {
    if (!selected?.availability || !serverId || !operationPreview?.confirmationToken) return;
    setOperationPreview(null);
    setRunning(true);
    setEvents([]);
    setDetails(null);
    try {
      await api.confirmOperation(
        serverId,
        {
          taskId: selected.availability.definition.id,
          taskVersion: selected.availability.definition.version,
          parameters,
          confirmationToken: operationPreview.confirmationToken,
        },
        appendEvent,
      );
    } catch (cause) {
      setError(describeTaskError(cause));
    } finally {
      setRunning(false);
    }
  }

  async function previewRemediation() {
    if (!selected?.availability || !serverId || selected.state !== 'remediable') return;
    setRemediationBusy(true);
    setError(null);
    try {
      setRemediationPreview(await api.previewTaskRemediation(serverId, selected.availability.definition.id));
    } catch (cause) {
      setError(describeTaskError(cause));
    } finally {
      setRemediationBusy(false);
    }
  }

  async function confirmRemediation() {
    if (!remediationPreview || !serverId) return;
    setRemediationBusy(true);
    setEvents([]);
    setDetails(null);
    setError(null);
    try {
      await api.confirmTaskRemediation(
        serverId,
        {
          previewId: remediationPreview.previewId,
          confirmationToken: remediationPreview.confirmationToken,
        },
        appendEvent,
      );
      setRemediationPreview(null);
      setTasks(await api.listTaskDefinitions(serverId));
    } catch (cause) {
      setError(describeTaskError(cause));
    } finally {
      setRemediationBusy(false);
    }
  }

  function appendEvent(event: ExecutionEvent) {
    setEvents((current) => [...current, event].slice(-500));
  }

  async function cancel() {
    const started = events.find((event) => event.type === 'started');
    if (started?.type === 'started') await api.cancelExecution(started.executionId);
  }

  return (
    <section className="task-page" aria-labelledby="tasks-title">
      <header className="page-heading task-page__heading">
        <div>
          <span className="eyebrow">统一工具库 · 自动匹配服务器能力</span>
          <h1 id="tasks-title">快捷任务</h1>
          <p>按你能理解的问题查找工具，不需要先知道 Linux 命令和发行版差异。</p>
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
        <button type="button" className={view === 'library' ? 'is-active' : ''} onClick={() => setView('library')}>工具库</button>
        <button type="button" className={view === 'scripts' ? 'is-active' : ''} onClick={() => setView('scripts')}>我的脚本</button>
        <button type="button" className={view === 'advanced' ? 'is-active' : ''} onClick={() => setView('advanced')}>高级执行</button>
      </div>

      {error && (
        <div className="inline-message inline-message--error page-alert task-error" role="alert">
          <strong>{error.summary}</strong>
          {error.retryable && <span>你可以修正后再次运行，客户端不会自动执行其他命令。</span>}
          {error.detail && <details><summary>查看技术详情</summary><code>{error.detail}</code></details>}
        </div>
      )}

      {view === 'scripts' ? (
        <ScriptCenter apiClient={api} servers={servers} serverId={serverId} builtInTasks={tasks} onChooseBuiltIn={() => setView('library')} />
      ) : view === 'advanced' ? (
        <AdvancedExecutionPanel servers={servers} serverId={serverId} />
      ) : loading ? (
        <div className="silver-card loading-card" role="status"><SpinnerGap className="spin" />正在读取服务器能力和工具…</div>
      ) : servers.length === 0 ? (
        <article className="silver-card milestone-notice"><Lightning weight="duotone" /><div><h2>请先添加服务器</h2><p>工具库需要目标服务器和已信任的 SSH 主机身份。</p></div></article>
      ) : (
        <>
          <ToolLibraryFilters query={query} showUnavailable={showUnavailable} onQueryChange={setQuery} onToggleUnavailable={() => setShowUnavailable((current) => !current)} />
          <div className="tool-library-workspace">
            <ToolCategoryRail selected={category} counts={counts} total={libraryItems.length} onSelect={setCategory} />
            <ToolCatalogList items={visibleItems} selectedKey={selectedKey} onSelect={chooseItem} />
            <ToolDetailPane
              item={selected}
              parameters={parameters}
              events={events}
              details={details}
              running={running}
              onParameterChange={(name, value) => setParameters((current) => ({ ...current, [name]: value }))}
              onRun={() => void requestRun()}
              onCancel={() => void cancel()}
              onRemediate={() => void previewRemediation()}
            />
          </div>
        </>
      )}

      {operationPreview && selectedServer && selected && (
        <div className="dialog-backdrop" role="presentation">
          <section className="silver-card modal-card danger-confirm" role="dialog" aria-modal="true" aria-labelledby="danger-confirm-title">
            <div><span className="eyebrow">预演完成 · 二次确认</span><h2 id="danger-confirm-title">确认危险操作</h2></div>
            <dl>
              <div><dt>目标服务器</dt><dd>{selectedServer.name}</dd></div>
              <div><dt>准备执行</dt><dd>{selected.title} · {selected.description}</dd></div>
              <div><dt>当前状态</dt><dd>{operationPreview.currentStateSummary}</dd></div>
              <div><dt>执行目标</dt><dd>{operationPreview.targetStateSummary}</dd></div>
              <div><dt>备份保护</dt><dd>{operationPreview.backupSummary.join('；') || '无可用备份'}</dd></div>
            </dl>
            <div className="modal-actions">
              <button className="secondary-button" type="button" onClick={() => setOperationPreview(null)}>取消</button>
              <button className="danger-button" type="button" onClick={() => void confirmDangerousOperation()}>确认并运行</button>
            </div>
          </section>
        </div>
      )}

      {remediationPreview && selectedServer && (
        <TaskRemediationDialog
          preview={remediationPreview}
          serverName={selectedServer.name}
          taskTitle={selected?.title ?? remediationPreview.taskId}
          busy={remediationBusy}
          onCancel={() => setRemediationPreview(null)}
          onConfirm={() => void confirmRemediation()}
        />
      )}

      {scriptRunDetails && selectedServer && (
        <ScriptRunDialog
          apiClient={api}
          script={scriptRunDetails}
          serverId={serverId}
          serverName={selectedServer.name}
          onClose={() => setScriptRunDetails(null)}
          onComplete={() => undefined}
        />
      )}
    </section>
  );
}
