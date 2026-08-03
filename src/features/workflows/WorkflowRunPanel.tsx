import {
  ArrowClockwise,
  Broom,
  DownloadSimple,
  Play,
  ShieldWarning,
  StopCircle,
  UploadSimple,
} from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import { asAppError, api } from '../../api/tauri';
import type {
  ExecutionFile,
  ServerProfile,
  WorkflowDraft,
  WorkflowEvent,
  WorkflowRunDetails,
  WorkflowRunRecord,
  WorkflowValidationReport,
} from '../../api/contracts';
import { summarizeWorkflowRisks } from './WorkflowInspector';
import { WorkflowTimeline } from './WorkflowTimeline';

type Confirmation = 'run' | 'retry' | 'rollback' | null;

const runLabels = {
  queued: '排队中', running: '运行中', paused: '已暂停', succeeded: '运行成功', cancelled: '已取消',
  uncertain: '状态不确定', rolled_back: '已回滚', rollback_failed: '回滚失败',
} as const;

export function WorkflowRunPanel({
  draft,
  workflowVersion,
  dirty,
  onValidation,
}: {
  draft: WorkflowDraft;
  workflowVersion: number | null;
  dirty: boolean;
  onValidation: (report: WorkflowValidationReport) => void;
}) {
  const [servers, setServers] = useState<ServerProfile[]>([]);
  const [serverId, setServerId] = useState('');
  const [runs, setRuns] = useState<WorkflowRunRecord[]>([]);
  const [details, setDetails] = useState<WorkflowRunDetails | null>(null);
  const [liveEvents, setLiveEvents] = useState<WorkflowEvent[]>([]);
  const [confirmation, setConfirmation] = useState<Confirmation>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  const [diagnostic, setDiagnostic] = useState<ExecutionFile | null>(null);
  const risks = summarizeWorkflowRisks(draft);

  useEffect(() => {
    let active = true;
    void api.listServers().then((profiles) => {
      if (!active) return;
      setServers(profiles);
      setServerId((current) => current || profiles[0]?.id || '');
    }).catch((error) => active && setMessage(asAppError(error).message));
    return () => { active = false; };
  }, []);

  useEffect(() => {
    let active = true;
    setDetails(null);
    setRuns([]);
    setDiagnostic(null);
    if (!draft.id) return () => { active = false; };
    void api.listWorkflowRuns({ workflowId: draft.id }).then(async (records) => {
      if (!active) return;
      setRuns(records);
      if (records[0]) {
        const restored = await api.getWorkflowRun(records[0].id);
        if (active) setDetails(restored);
      }
    }).catch((error) => active && setMessage(asAppError(error).message));
    return () => { active = false; };
  }, [draft.id]);

  const receiveEvent = (event: WorkflowEvent) => {
    setLiveEvents((current) => current.some((item) => item.sequence === event.sequence)
      ? current
      : [...current, event].sort((left, right) => left.sequence - right.sequence));
  };

  const remember = (next: WorkflowRunDetails) => {
    setDetails(next);
    setRuns((current) => [next.run, ...current.filter((run) => run.id !== next.run.id)]);
  };

  const performStart = async (dangerousConfirmed: boolean) => {
    if (!draft.id || workflowVersion === null || !serverId) return;
    setBusy(true);
    setConfirmation(null);
    setMessage('');
    setDiagnostic(null);
    setLiveEvents([]);
    try {
      const next = await api.startWorkflowRun({
        workflowId: draft.id,
        workflowVersion,
        serverId,
        dangerousConfirmed,
      }, receiveEvent);
      remember(next);
    } catch (error) {
      setMessage(asAppError(error).message);
    } finally {
      setBusy(false);
    }
  };

  const preflight = async () => {
    if (!draft.id || workflowVersion === null) {
      setMessage('请先保存工作流，再运行对应的不可变版本。');
      return;
    }
    if (dirty) {
      setMessage('画布有未保存改动，请先保存新版本。');
      return;
    }
    if (!serverId) {
      setMessage('请先添加并选择服务器。');
      return;
    }
    setBusy(true);
    try {
      const report = await api.validateWorkflow(draft);
      onValidation(report);
      if (!report.valid) {
        setMessage(`运行前校验未通过：${report.diagnostics.length} 项问题。`);
        return;
      }
      if (risks.length > 0) setConfirmation('run');
      else await performStart(false);
    } catch (error) {
      setMessage(asAppError(error).message);
    } finally {
      setBusy(false);
    }
  };

  const retry = async (confirmed: boolean) => {
    if (!details) return;
    setBusy(true);
    setConfirmation(null);
    setMessage('');
    try {
      const next = await api.retryWorkflowNode(details.run.id, confirmed, receiveEvent);
      remember(next);
    } catch (error) {
      setMessage(asAppError(error).message);
    } finally {
      setBusy(false);
    }
  };

  const requestRetry = () => risks.length > 0 ? setConfirmation('retry') : void retry(false);

  const cancel = async () => {
    if (!details) return;
    setBusy(true);
    try {
      await api.cancelWorkflowRun(details.run.id);
      const reloaded = await api.getWorkflowRun(details.run.id);
      if (reloaded) remember(reloaded);
    } catch (error) {
      setMessage(asAppError(error).message);
    } finally {
      setBusy(false);
    }
  };

  const rollback = async () => {
    if (!details) return;
    setBusy(true);
    setConfirmation(null);
    try {
      remember(await api.rollbackWorkflowRun(details.run.id, true));
    } catch (error) {
      setMessage(asAppError(error).message);
    } finally {
      setBusy(false);
    }
  };

  const cleanup = async () => {
    if (!details) return;
    setBusy(true);
    try {
      const count = await api.cleanupWorkflowRestorePoints(details.run.id);
      setMessage(`已清理 ${count} 个恢复点。`);
      const reloaded = await api.getWorkflowRun(details.run.id);
      if (reloaded) remember(reloaded);
    } catch (error) {
      setMessage(asAppError(error).message);
    } finally {
      setBusy(false);
    }
  };

  const exportDiagnostic = async () => {
    if (!details) return;
    setBusy(true);
    try {
      setDiagnostic(await api.exportWorkflowDiagnostics(details.run.id));
    } catch (error) {
      setMessage(asAppError(error).message);
    } finally {
      setBusy(false);
    }
  };

  const selectRun = async (runId: string) => {
    const restored = await api.getWorkflowRun(runId);
    if (restored) remember(restored);
  };

  return (
    <section className="silver-card workflow-run-panel" aria-label="工作流运行控制">
      <header className="workflow-run-panel__header">
        <div>
          <span className="workflow-panel-kicker">运行与恢复</span>
          <strong>执行已保存的工作流版本</strong>
          <small>状态和终态仅来自 Rust 服务与持久化记录，不使用前端计时器推测。</small>
        </div>
        <div className="workflow-run-targets">
          <label>目标服务器<select aria-label="目标服务器" value={serverId} onChange={(event) => setServerId(event.target.value)}>
            {servers.length === 0 && <option value="">暂无服务器</option>}
            {servers.map((server) => <option key={server.id} value={server.id}>{server.name}</option>)}
          </select></label>
          <label>历史运行<select aria-label="历史运行" value={details?.run.id ?? ''} onChange={(event) => void selectRun(event.target.value)}>
            <option value="">当前未选择</option>
            {runs.map((run) => <option key={run.id} value={run.id}>{runLabels[run.status]} · {run.id}</option>)}
          </select></label>
          <button type="button" className="success-button" onClick={() => void preflight()} disabled={busy || !draft.id}>
            <Play weight="fill" aria-hidden="true" />运行工作流
          </button>
        </div>
      </header>

      {message && <div className="workflow-run-message" role="status">{message}</div>}
      {diagnostic && <div className="workflow-diagnostic-file"><DownloadSimple aria-hidden="true" /><span>诊断已生成</span><code>{diagnostic.relativePath}</code></div>}

      {details ? (
        <div className="workflow-run-content">
          <section className="workflow-run-summary">
            <span className={`workflow-run-status workflow-run-status--${details.run.status}`}>{runLabels[details.run.status]}</span>
            <strong>{details.run.id}</strong>
            <small>工作流 v{details.run.workflowVersion} · 事件 {Math.max(details.events.length, liveEvents.length)} 条</small>
            {details.run.errorMessage && <p>{details.run.errorMessage}</p>}
            {details.run.status === 'uncertain' && <p className="workflow-uncertain-note">请先核验远端状态，再决定回滚或人工处理；系统不会显示虚假的取消成功。</p>}
            <div className="workflow-run-actions">
              {details.run.status === 'paused' && details.run.retryable && (
                <button type="button" className="primary-button" onClick={requestRetry} disabled={busy}><ArrowClockwise aria-hidden="true" />重试失败节点</button>
              )}
              {details.run.status === 'running' && (
                <button type="button" className="danger-button" onClick={() => void cancel()} disabled={busy}><StopCircle aria-hidden="true" />取消运行</button>
              )}
              {details.run.status !== 'running' && details.restorePoints.some((point) => point.status === 'available') && (
                <button type="button" className="danger-button" onClick={() => setConfirmation('rollback')} disabled={busy}><UploadSimple aria-hidden="true" />回滚恢复点</button>
              )}
              {details.run.status !== 'running' && details.restorePoints.length > 0 && (
                <button type="button" className="secondary-button" onClick={() => void cleanup()} disabled={busy}><Broom aria-hidden="true" />清理恢复点</button>
              )}
              <button type="button" className="secondary-button" onClick={() => void exportDiagnostic()} disabled={busy}><DownloadSimple aria-hidden="true" />导出诊断</button>
            </div>
          </section>
          <WorkflowTimeline details={details} draft={draft} />
        </div>
      ) : (
        <div className="workflow-run-empty">选择服务器并运行；已有记录会在重载后从数据库恢复。</div>
      )}

      {confirmation && (
        <div className="dialog-backdrop">
          <section className="silver-card modal-card workflow-confirm-dialog" role="dialog" aria-modal="true" aria-label={confirmation === 'rollback' ? '确认回滚工作流' : '确认工作流危险操作'}>
            <header><ShieldWarning weight="fill" aria-hidden="true" /><div><h2>{confirmation === 'rollback' ? '确认回滚工作流' : '确认工作流危险操作'}</h2><p>{confirmation === 'rollback' ? '将按成功变更的逆序恢复或删除远端文件。' : '只展示目标与影响摘要，不回显命令或脚本正文。'}</p></div></header>
            {confirmation === 'rollback' ? (
              <ul>{details?.restorePoints.filter((point) => point.status === 'available').map((point) => <li key={point.id}><strong>{point.originalExisted ? '恢复已有文件' : '删除本次新建文件'}</strong><code>{point.remotePath}</code></li>)}</ul>
            ) : (
              <ul>{risks.map((risk) => <li key={risk.nodeId}><strong>{risk.title}</strong><span>{risk.detail}</span></li>)}</ul>
            )}
            <div className="modal-actions">
              <button type="button" className="secondary-button" onClick={() => setConfirmation(null)}>返回检查</button>
              <button type="button" className="danger-button" onClick={() => confirmation === 'rollback' ? void rollback() : confirmation === 'retry' ? void retry(true) : void performStart(true)}>
                {confirmation === 'rollback' ? '确认回滚' : confirmation === 'retry' ? '确认并重试' : '确认并运行'}
              </button>
            </div>
          </section>
        </div>
      )}
    </section>
  );
}
