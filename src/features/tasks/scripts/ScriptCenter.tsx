import { FileArrowDown, FileCode, Plus, ShieldCheck } from '@phosphor-icons/react';
import { useEffect, useMemo, useRef, useState } from 'react';

import { normalizeAppError } from '../../../api/errors';
import type {
  PersonalScriptDetails,
  PersonalScriptRunResult,
  PersonalScriptSummary,
  PersonalScriptVersion,
  ServerProfile,
  TaskAvailability,
} from '../../../api/contracts';
import { ScriptEditor } from './ScriptEditor';
import { ScriptList } from './ScriptList';
import { ScriptRunDialog } from './ScriptRunDialog';
import type { PersonalScriptApi, ScriptEditorDraft } from './types';

interface ScriptCenterProps {
  apiClient: PersonalScriptApi;
  servers: ServerProfile[];
  serverId: string;
  builtInTasks?: TaskAvailability[];
  onChooseBuiltIn?: () => void;
  onDirtyChange?: (dirty: boolean) => void;
}

const emptyDraft = (): ScriptEditorDraft => ({
  title: '',
  category: '个人脚本',
  tags: [],
  body: '#!/bin/sh\nset -eu\n\n',
  parameters: [],
  timeoutSeconds: 300,
  shell: 'posix_sh',
});

function draftFrom(details: PersonalScriptDetails): ScriptEditorDraft {
  return {
    title: details.definition.title,
    category: details.definition.category,
    tags: details.definition.tags,
    body: details.activeVersion.body,
    parameters: details.activeVersion.parameters,
    timeoutSeconds: details.activeVersion.timeoutSeconds,
    shell: details.activeVersion.shell,
  };
}

export function ScriptCenter({ apiClient, servers, serverId, builtInTasks = [], onChooseBuiltIn, onDirtyChange }: ScriptCenterProps) {
  const [scope, setScope] = useState<'personal' | 'builtin'>('personal');
  const [scripts, setScripts] = useState<PersonalScriptSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [details, setDetails] = useState<PersonalScriptDetails | null>(null);
  const [versions, setVersions] = useState<PersonalScriptVersion[]>([]);
  const [draft, setDraft] = useState<ScriptEditorDraft>(emptyDraft);
  const [dirty, setDirty] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [query, setQuery] = useState('');
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [feedback, setFeedback] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [runOpen, setRunOpen] = useState(false);
  const importInput = useRef<HTMLInputElement>(null);

  async function loadList(preferredId?: string | null) {
    setLoading(true);
    try {
      const rows = await apiClient.listPersonalScripts({
        query: query || undefined,
        favorite: favoritesOnly || undefined,
      });
      setScripts(rows);
      const nextId = preferredId === undefined ? selectedId : preferredId;
      if (nextId && rows.some((row) => row.id === nextId)) setSelectedId(nextId);
      else if (selectedId && !rows.some((row) => row.id === selectedId)) {
        setSelectedId(null);
        setDetails(null);
      }
    } catch (cause) {
      setError(normalizeAppError(cause).message);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void loadList();
    // The explicit search button avoids remote requests on every keystroke.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [favoritesOnly]);

  useEffect(() => {
    onDirtyChange?.(dirty);
    if (!dirty) return undefined;
    const protectUnsavedChanges = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = '';
    };
    window.addEventListener('beforeunload', protectUnsavedChanges);
    return () => window.removeEventListener('beforeunload', protectUnsavedChanges);
  }, [dirty, onDirtyChange]);

  async function selectScript(scriptId: string) {
    if (dirty && !window.confirm('当前修改尚未保存，确定离开吗？')) return;
    setError(null);
    setFeedback(null);
    const [next, history] = await Promise.all([
      apiClient.getPersonalScriptForEditor(scriptId),
      apiClient.listPersonalScriptVersions(scriptId),
    ]);
    if (!next) return;
    setSelectedId(scriptId);
    setDetails(next);
    setVersions(history);
    setDraft(draftFrom(next));
    setDirty(false);
  }

  function startNew() {
    if (dirty && !window.confirm('当前修改尚未保存，确定新建吗？')) return;
    setSelectedId(null);
    setDetails(null);
    setVersions([]);
    setDraft(emptyDraft());
    setDirty(true);
    setFeedback(null);
    setError(null);
  }

  async function save() {
    setSaving(true);
    setError(null);
    try {
      let saved: PersonalScriptDetails;
      if (!details) {
        saved = await apiClient.createPersonalScript(draft);
      } else {
        await apiClient.updatePersonalScriptMetadata(details.definition.id, {
          title: draft.title,
          category: draft.category,
          tags: draft.tags,
        });
        await apiClient.savePersonalScriptVersion(details.definition.id, {
          body: draft.body,
          parameters: draft.parameters,
          timeoutSeconds: draft.timeoutSeconds,
          shell: draft.shell,
        });
        const refreshed = await apiClient.getPersonalScriptForEditor(details.definition.id);
        if (!refreshed) throw new Error('保存后无法重新读取脚本');
        saved = refreshed;
      }
      setDetails(saved);
      setSelectedId(saved.definition.id);
      setDraft(draftFrom(saved));
      setDirty(false);
      setVersions(await apiClient.listPersonalScriptVersions(saved.definition.id));
      setFeedback(details ? '已保存为新的不可变版本。' : '脚本已创建，检查内容后请手动启用。');
      await loadList(saved.definition.id);
    } catch (cause) {
      setError(normalizeAppError(cause).message);
    } finally {
      setSaving(false);
    }
  }

  async function copy(scriptId: string) {
    try {
      const copied = await apiClient.copyPersonalScript(scriptId);
      await loadList(copied.definition.id);
      await selectScript(copied.definition.id);
      setFeedback('已复制为新的未启用脚本。');
    } catch (cause) {
      setError(normalizeAppError(cause).message);
    }
  }

  async function toggleEnabled() {
    if (!details) return;
    await apiClient.setPersonalScriptEnabled(details.definition.id, !details.definition.isEnabled);
    const refreshed = await apiClient.getPersonalScriptForEditor(details.definition.id);
    if (refreshed) {
      setDetails(refreshed);
      setFeedback(refreshed.definition.isEnabled ? '脚本已启用，运行时仍需二次确认。' : '脚本已停用。');
    }
    await loadList(details.definition.id);
  }

  async function toggleFavorite() {
    if (!details) return;
    await apiClient.setPersonalScriptFavorite(details.definition.id, !details.definition.isFavorite);
    const refreshed = await apiClient.getPersonalScriptForEditor(details.definition.id);
    if (refreshed) setDetails(refreshed);
    await loadList(details.definition.id);
  }

  async function remove() {
    if (!details || !window.confirm(`确定删除“${details.definition.title}”吗？历史运行仍会保留。`)) return;
    await apiClient.deletePersonalScript(details.definition.id);
    setSelectedId(null);
    setDetails(null);
    setVersions([]);
    setDraft(emptyDraft());
    setDirty(false);
    setFeedback('脚本已删除，历史运行记录仍然保留。');
    await loadList(null);
  }

  async function importFile(file: File | undefined) {
    if (!file) return;
    try {
      const imported = await apiClient.importPersonalScript(await file.text());
      await loadList(imported.definition.id);
      await selectScript(imported.definition.id);
      setFeedback('已导入，默认未启用');
    } catch (cause) {
      setError(normalizeAppError(cause).message);
    } finally {
      if (importInput.current) importInput.current.value = '';
    }
  }

  async function exportCurrent() {
    if (!details) return;
    const exported = await apiClient.exportPersonalScript(details.definition.id);
    setFeedback(`已导出到项目数据目录：${exported.relativePath}`);
  }

  function changeDraft(next: ScriptEditorDraft) {
    setDraft(next);
    setDirty(true);
    setFeedback(null);
  }

  const selectedServer = useMemo(
    () => servers.find((server) => server.id === serverId) ?? null,
    [serverId, servers],
  );

  return (
    <section className="script-center" aria-label="个人脚本中心">
      <div className="script-scope-switch" aria-label="脚本类型">
        <button type="button" className={scope === 'builtin' ? 'is-active' : ''} onClick={() => setScope('builtin')}>内置审核脚本</button>
        <button type="button" className={scope === 'personal' ? 'is-active' : ''} onClick={() => setScope('personal')}>我的脚本</button>
      </div>

      {scope === 'builtin' ? (
        <section className="silver-card builtin-script-panel">
          <header><ShieldCheck weight="duotone" /><div><span className="eyebrow">只读 · 随客户端审核发布</span><h2>内置审核脚本</h2></div></header>
          <p>内置能力由后端固定参数和兼容性规则生成，不向界面暴露原始命令。请选择对应内置任务运行。</p>
          <div>{builtInTasks.filter((task) => task.definition.category === 'script' || task.definition.category === 'advanced').map((task) => <span className="script-template-chip" key={task.definition.id}>{task.definition.title}</span>)}</div>
          <button className="primary-button" type="button" onClick={onChooseBuiltIn}>前往内置任务</button>
        </section>
      ) : (
        <>
          <header className="script-center-toolbar silver-card">
            <div className="script-search"><input aria-label="搜索个人脚本" placeholder="按名称或分类搜索" value={query} onChange={(event) => setQuery(event.target.value)} /><button className="secondary-button" type="button" onClick={() => void loadList()}>搜索</button></div>
            <label className="checkbox-field"><input type="checkbox" checked={favoritesOnly} onChange={(event) => setFavoritesOnly(event.target.checked)} /><span>只看收藏</span></label>
            <button className="secondary-button" type="button" onClick={() => importInput.current?.click()}><FileArrowDown />导入脚本</button>
            <input ref={importInput} className="visually-hidden" aria-label="选择脚本包" type="file" accept="application/json,.json" onChange={(event) => void importFile(event.target.files?.[0])} />
            <button className="primary-button" type="button" onClick={startNew}><Plus />新建脚本</button>
          </header>

          {feedback && <div className="inline-message inline-message--success script-feedback" role="status">{feedback}</div>}
          {error && <div className="inline-message inline-message--error script-feedback" role="alert">{error}</div>}

          <div className="script-center-workspace">
            <aside className="silver-card script-list-pane" aria-label="脚本列表区域"><header><FileCode weight="duotone" /><div><span className="eyebrow">分类 · 收藏 · 启停</span><h2>个人脚本</h2></div></header><ScriptList scripts={scripts} selectedId={selectedId} loading={loading} onSelect={(id) => void selectScript(id)} onCopy={(id) => void copy(id)} /></aside>
            <ScriptEditor details={details} versions={versions} draft={draft} dirty={dirty} saving={saving} serverReady={Boolean(selectedServer)} onChange={changeDraft} onSave={() => void save()} onRun={() => setRunOpen(true)} onToggleEnabled={() => void toggleEnabled()} onToggleFavorite={() => void toggleFavorite()} onExport={() => void exportCurrent()} onDelete={() => void remove()} />
          </div>
        </>
      )}

      {runOpen && details && selectedServer && <ScriptRunDialog apiClient={apiClient} script={details} serverId={selectedServer.id} serverName={selectedServer.name} onClose={() => setRunOpen(false)} onComplete={(_result: PersonalScriptRunResult) => setFeedback('脚本运行已结束，可到执行记录查看详情。')} />}
    </section>
  );
}
