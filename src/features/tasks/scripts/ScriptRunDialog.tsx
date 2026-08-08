import { Play, ShieldWarning, SpinnerGap, X } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import { normalizeAppError } from '../../../api/errors';
import type {
  ExecutionEvent,
  PersonalScriptDetails,
  PersonalScriptRunPreview,
  PersonalScriptRunResult,
} from '../../../api/contracts';
import { executionStatusLabel } from '../../../presentation/executionLabels';
import { ParameterForm } from '../ParameterForm';
import type { PersonalScriptApi } from './types';

interface ScriptRunDialogProps {
  apiClient: PersonalScriptApi;
  script: PersonalScriptDetails;
  serverId: string;
  serverName: string;
  onClose: () => void;
  onComplete: (result: PersonalScriptRunResult) => void;
}

export function ScriptRunDialog({ apiClient, script, serverId, serverName, onClose, onComplete }: ScriptRunDialogProps) {
  const definitions = script.activeVersion.parameters;
  const [values, setValues] = useState<Record<string, unknown>>(() =>
    Object.fromEntries(definitions.filter((parameter) => parameter.defaultValue !== null).map((parameter) => [parameter.name, parameter.defaultValue])),
  );
  const [preview, setPreview] = useState<PersonalScriptRunPreview | null>(null);
  const [events, setEvents] = useState<ExecutionEvent[]>([]);
  const [result, setResult] = useState<PersonalScriptRunResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function createPreview() {
    setBusy(true);
    setError(null);
    try {
      setPreview(await apiClient.previewPersonalScriptRun(script.definition.id, serverId, values));
    } catch (cause) {
      setError(normalizeAppError(cause).message);
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    if (definitions.length === 0) void createPreview();
    // A new dialog instance is mounted for each selected script.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [script.definition.id]);

  async function confirm() {
    if (!preview) return;
    setBusy(true);
    setError(null);
    setEvents([]);
    try {
      const completed = await apiClient.confirmPersonalScriptRun(
        { previewId: preview.previewId, confirmationToken: preview.confirmationToken },
        (event) => setEvents((current) => [...current, event].slice(-500)),
      );
      setResult(completed);
      onComplete(completed);
    } catch (cause) {
      setError(normalizeAppError(cause).message);
    } finally {
      setBusy(false);
    }
  }

  async function close() {
    if (preview && !result) await apiClient.cancelPersonalScriptRun(preview.previewId).catch(() => undefined);
    onClose();
  }

  return (
    <div className="dialog-backdrop" role="presentation">
      <section className="silver-card modal-card script-run-dialog" role="dialog" aria-modal="true" aria-labelledby="script-run-title">
        <header>
          <div><span className="eyebrow">单服务器 · 受控执行</span><h2 id="script-run-title">运行个人脚本</h2></div>
          <button className="icon-button" type="button" aria-label="关闭运行窗口" onClick={() => void close()}><X /></button>
        </header>
        <dl className="script-run-facts">
          <div><dt>脚本</dt><dd>{script.definition.title} · v{script.activeVersion.versionNumber}</dd></div>
          <div><dt>目标服务器</dt><dd>{serverName}</dd></div>
          <div><dt>Shell</dt><dd>{script.activeVersion.shell === 'posix_sh' ? 'POSIX sh' : script.activeVersion.shell === 'bash' ? 'Bash' : 'PowerShell'}</dd></div>
        </dl>

        {!preview && (
          <form onSubmit={(event) => { event.preventDefault(); void createPreview(); }}>
            <ParameterForm definitions={definitions} values={values} onChange={(name, value) => setValues((current) => ({ ...current, [name]: value }))} />
            <button className="primary-button" type="submit" disabled={busy}>{busy ? <SpinnerGap className="spin" /> : <ShieldWarning />}检查并继续</button>
          </form>
        )}

        {preview && !result && (
          <div className="script-run-preview">
            <div className="script-run-warning"><ShieldWarning weight="fill" /><div><strong>{preview.warning}</strong><span>静态扫描只能提示风险，不能证明脚本安全。</span></div></div>
            <dl>
              <div><dt>正文摘要</dt><dd>{preview.lineCount} 行 · {preview.characterCount} 字符</dd></div>
              <div><dt>SHA-256</dt><dd><code>{preview.bodySha256.slice(0, 16)}…</code></dd></div>
              <div><dt>超时</dt><dd>{preview.timeoutSeconds} 秒</dd></div>
              <div><dt>Shell</dt><dd>{preview.shell === 'posix_sh' ? 'POSIX sh' : preview.shell === 'bash' ? 'Bash' : 'PowerShell'}</dd></div>
              <div><dt>扫描提示</dt><dd>{preview.scanWarnings.length} 项</dd></div>
            </dl>
            {preview.scanWarnings.length > 0 && <ul>{preview.scanWarnings.map((warning) => <li key={warning.code}>第 {warning.lineNumber} 行：{warning.message}</li>)}</ul>}
            <button className="danger-button" type="button" disabled={busy} onClick={() => void confirm()}>{busy ? <SpinnerGap className="spin" /> : <Play weight="fill" />}确认并运行</button>
          </div>
        )}

        {result && <div className={`inline-message ${result.execution.record.status === 'succeeded' ? 'inline-message--success' : 'inline-message--error'}`}><strong>运行状态：{executionStatusLabel(result.execution.record.status)}</strong><span>完整输出可在执行记录中查看。</span></div>}
        {events.length > 0 && <pre className="script-run-output">{events.filter((event) => event.type === 'stdout' || event.type === 'stderr').map((event) => event.type === 'stdout' || event.type === 'stderr' ? event.text : '').join('')}</pre>}
        {error && <div className="inline-message inline-message--error" role="alert">{error}</div>}
        <footer className="modal-actions"><button className="secondary-button" type="button" onClick={() => void close()}>{result ? '完成' : '取消'}</button></footer>
      </section>
    </div>
  );
}
