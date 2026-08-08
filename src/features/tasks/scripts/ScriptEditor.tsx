import { FloppyDisk, Play, Plus, Star, Trash, UploadSimple, Warning } from '@phosphor-icons/react';
import { useMemo } from 'react';

import type {
  ParameterDefinition,
  ParameterKind,
  PersonalScriptDetails,
  PersonalScriptVersion,
} from '../../../api/contracts';
import type { ScriptEditorDraft } from './types';
import { analyzeScript } from './scriptAnalysis';

type ScriptParameterType = ParameterKind['type'];

interface ScriptEditorProps {
  details: PersonalScriptDetails | null;
  versions: PersonalScriptVersion[];
  draft: ScriptEditorDraft;
  dirty: boolean;
  saving: boolean;
  serverReady: boolean;
  onChange: (draft: ScriptEditorDraft) => void;
  onSave: () => void;
  onRun: () => void;
  onToggleEnabled: () => void;
  onToggleFavorite: () => void;
  onExport: () => void;
  onDelete: () => void;
}

const parameterTypes: Array<{ value: ScriptParameterType; label: string }> = [
  { value: 'string', label: '文本' },
  { value: 'integer', label: '整数' },
  { value: 'boolean', label: '开关' },
  { value: 'host', label: '主机地址' },
  { value: 'port', label: '端口' },
  { value: 'serviceName', label: '服务名' },
  { value: 'containerName', label: '容器名' },
  { value: 'absolutePath', label: '远程绝对路径' },
];

function parameterKind(type: ScriptParameterType): ParameterKind {
  if (type === 'string') return { type, minLength: 0, maxLength: 4096, multiline: false };
  if (type === 'integer') return { type, min: -2_147_483_648, max: 2_147_483_647 };
  if (type === 'enum') return { type, options: ['选项一'] };
  return { type } as ParameterKind;
}

function newParameter(index: number): ParameterDefinition {
  return {
    name: `PARAM_${index + 1}`,
    label: `参数${index + 1}`,
    description: '运行脚本前填写',
    kind: parameterKind('string'),
    required: true,
    defaultValue: null,
    sensitive: false,
  };
}

export function ScriptEditor({
  details,
  versions,
  draft,
  dirty,
  saving,
  serverReady,
  onChange,
  onSave,
  onRun,
  onToggleEnabled,
  onToggleFavorite,
  onExport,
  onDelete,
}: ScriptEditorProps) {
  const advisoryWarnings = useMemo(
    () => analyzeScript(draft.shell, draft.body),
    [draft.body, draft.shell],
  );
  function patchParameter(index: number, patch: Partial<ParameterDefinition>) {
    onChange({
      ...draft,
      parameters: draft.parameters.map((parameter, current) =>
        current === index ? { ...parameter, ...patch } : parameter),
    });
  }

  return (
    <section className="silver-card script-editor" aria-label="脚本编辑器">
      <header className="script-editor__header">
        <div>
          <span className="eyebrow">{details ? `个人脚本 · v${details.activeVersion.versionNumber}` : '新建个人脚本'}</span>
          <h2>{details?.definition.title ?? '创建脚本'}</h2>
        </div>
        <span className="risk-chip risk-chip--dangerous">始终高风险</span>
      </header>

      <div className="script-editor__scroll">
        <div className="script-meta-grid">
          <label>脚本名称<input aria-label="脚本名称" value={draft.title} maxLength={80} onChange={(event) => onChange({ ...draft, title: event.target.value })} /></label>
          <label>分类<input aria-label="脚本分类" value={draft.category} maxLength={40} onChange={(event) => onChange({ ...draft, category: event.target.value })} /></label>
          <label className="script-tags-field">标签（用逗号分隔）<input aria-label="脚本标签" value={draft.tags.join(', ')} onChange={(event) => onChange({ ...draft, tags: event.target.value.split(/[,，]/).map((tag) => tag.trim()).filter(Boolean).slice(0, 20) })} /></label>
          <label>运行 Shell<select aria-label="脚本 Shell" value={draft.shell} onChange={(event) => onChange({ ...draft, shell: event.target.value as ScriptEditorDraft['shell'] })}><option value="posix_sh">POSIX sh</option><option value="bash">Bash</option><option value="powershell">PowerShell</option></select></label>
          <label>超时时间（秒）<input aria-label="脚本超时时间" type="number" min={1} max={3600} value={draft.timeoutSeconds} onChange={(event) => onChange({ ...draft, timeoutSeconds: Number(event.target.value) })} /></label>
        </div>

        <label className="script-body-field">
          <span>{draft.shell === 'powershell' ? 'PowerShell 脚本正文' : draft.shell === 'bash' ? 'Bash 脚本正文' : 'POSIX sh 脚本正文'}</span>
          <textarea aria-label="脚本正文" spellCheck={false} value={draft.body} onChange={(event) => onChange({ ...draft, body: event.target.value })} />
          <small>{new Blob([draft.body]).size.toLocaleString()} / 1,048,576 字节；参数通过进程环境变量 QZ_PARAM_参数名读取，不做源码替换。</small>
        </label>

        {advisoryWarnings.length > 0 && <section className="script-advisory" aria-label="脚本静态提示"><header><Warning weight="fill" /><strong>静态风险提示（{advisoryWarnings.length}）</strong></header><ul>{advisoryWarnings.map((warning) => <li key={warning.code}>第 {warning.lineNumber} 行：{warning.message}</li>)}</ul><small>提示用于辅助审查，不会把个人脚本标记为安全。</small></section>}

        <section className="script-parameter-builder" aria-labelledby="script-parameters-title">
          <header><div><span className="eyebrow">强类型参数</span><h3 id="script-parameters-title">运行前让用户填写</h3></div><button className="secondary-button" type="button" onClick={() => onChange({ ...draft, parameters: [...draft.parameters, newParameter(draft.parameters.length)] })}><Plus />添加参数</button></header>
          {draft.parameters.length === 0 ? <p className="parameter-empty">这个脚本不需要运行参数。</p> : draft.parameters.map((parameter, index) => (
            <div className="script-parameter-row" key={`${parameter.name}-${index}`}>
              <label>变量名<input aria-label={`参数 ${index + 1} 变量名`} value={parameter.name} onChange={(event) => patchParameter(index, { name: event.target.value.toUpperCase() })} /></label>
              <label>中文名称<input aria-label={`参数 ${index + 1} 中文名称`} value={parameter.label} onChange={(event) => patchParameter(index, { label: event.target.value })} /></label>
              <label>类型<select aria-label={`参数 ${index + 1} 类型`} value={parameter.kind.type} onChange={(event) => patchParameter(index, { kind: parameterKind(event.target.value as ScriptParameterType) })}>{parameterTypes.map((type) => <option key={type.value} value={type.value}>{type.label}</option>)}</select></label>
              <label className="checkbox-field"><input aria-label={`参数 ${index + 1} 必填`} type="checkbox" checked={parameter.required} onChange={(event) => patchParameter(index, { required: event.target.checked })} /><span>必填</span></label>
              <label className="checkbox-field"><input aria-label={`参数 ${index + 1} 敏感`} type="checkbox" checked={parameter.sensitive} onChange={(event) => patchParameter(index, { sensitive: event.target.checked })} /><span>敏感值</span></label>
              <button type="button" className="icon-button" aria-label={`删除参数 ${index + 1}`} onClick={() => onChange({ ...draft, parameters: draft.parameters.filter((_, current) => current !== index) })}><Trash /></button>
            </div>
          ))}
        </section>

        {versions.length > 0 && (
          <details className="script-version-history">
            <summary>历史版本（{versions.length}）</summary>
            <ol>{versions.map((version) => <li key={version.id}><strong>v{version.versionNumber} · {version.shell === 'posix_sh' ? 'POSIX sh' : version.shell === 'bash' ? 'Bash' : 'PowerShell'}</strong><span>{version.bodySha256.slice(0, 12)}…</span><time>{new Date(version.createdAt).toLocaleString()}</time></li>)}</ol>
          </details>
        )}
      </div>

      <footer className="script-editor__actions">
        <button className="primary-button" type="button" disabled={saving || !dirty} onClick={onSave}><FloppyDisk />{saving ? '正在保存…' : details ? '保存新版本' : '创建脚本'}</button>
        {details && <button className="secondary-button" type="button" onClick={onToggleEnabled}>{details.definition.isEnabled ? '停用脚本' : '启用脚本'}</button>}
        {details && <button className="secondary-button" type="button" onClick={onToggleFavorite}><Star weight={details.definition.isFavorite ? 'fill' : 'regular'} />{details.definition.isFavorite ? '取消收藏' : '收藏'}</button>}
        {details && <button className="secondary-button" type="button" onClick={onExport}><UploadSimple />导出</button>}
        {details && <button className="danger-button danger-button--quiet" type="button" onClick={onDelete}><Trash />删除</button>}
        {details && <button className="success-button script-run-button" type="button" disabled={!details.definition.isEnabled || !serverReady || dirty} onClick={onRun}><Play weight="fill" />运行脚本</button>}
      </footer>
    </section>
  );
}
