import { CheckCircle, FloppyDisk, FlowArrow, Plus, Trash } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import { asAppError, api } from '../../api/tauri';
import type { WorkflowDefinition, WorkflowDraft, WorkflowNode, WorkflowNodeConfig, WorkflowSummary, WorkflowValidationReport } from '../../api/contracts';
import { WorkflowCanvas } from './WorkflowCanvas';
import { WorkflowInspector } from './WorkflowInspector';
import { WorkflowLibrary, type WorkflowStepType } from './WorkflowLibrary';
import { WorkflowValidationPanel } from './WorkflowValidationPanel';
import { createReferenceWorkflowDraft } from './fixtures';
import './workflow.css';

let localNodeCounter = 0;

function nextNode(type: WorkflowStepType, index: number): WorkflowNode {
  const id = `local-${type}-${Date.now()}-${++localNodeCounter}`;
  const configs: Record<WorkflowStepType, WorkflowNodeConfig> = {
    start: { type: 'start' },
    task: { type: 'task', taskId: 'system.overview', taskVersion: 1, parameters: {} },
    custom: { type: 'custom', mode: 'command', content: '', timeoutSeconds: 30 },
    upload: { type: 'upload', localPath: '', remotePath: '', overwrite: false, createRestorePoint: true },
    download: { type: 'download', remotePath: '', suggestedName: '', overwrite: false },
    logSearch: {
      type: 'logSearch', path: '/var/log', keyword: '', caseSensitive: false, contextLines: 2,
      limit: 200, startTime: null, endTime: null,
    },
    condition: {
      type: 'condition', sourceNodeId: '', predicate: { kind: 'exitCode', operator: 'equal', value: 0 },
    },
    stop: { type: 'stop', message: '流程已停止' },
  };
  const labels: Record<WorkflowStepType, string> = {
    start: '开始', task: '快捷任务', custom: '自定义命令', upload: '上传文件', download: '下载文件',
    logSearch: '检索日志', condition: '条件判断', stop: '停止并提示',
  };
  return {
    id,
    name: `${labels[type]} ${index}`,
    position: { x: 540, y: 340 },
    config: configs[type],
  };
}

function toDraft(definition: WorkflowDefinition): WorkflowDraft {
  return {
    id: definition.id,
    name: definition.name,
    description: definition.description,
    nodes: definition.nodes,
    edges: definition.edges,
  };
}

export function WorkflowPage() {
  const [summaries, setSummaries] = useState<WorkflowSummary[]>([]);
  const [draft, setDraft] = useState<WorkflowDraft>(() => createReferenceWorkflowDraft());
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(draft.nodes[0]?.id ?? null);
  const [zoom, setZoom] = useState(1);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  const [validation, setValidation] = useState<WorkflowValidationReport | null>(null);

  const loadList = async () => {
    const list = await api.listWorkflows();
    setSummaries(list);
    return list;
  };

  const selectWorkflow = async (workflowId: string) => {
    setBusy(true);
    setMessage('');
    try {
      const definition = await api.getWorkflow(workflowId, null);
      if (definition) {
        setDraft(toDraft(definition));
        setValidation(null);
        setSelectedNodeId(definition.nodes[0]?.id ?? null);
      }
    } catch (error) {
      setMessage(asAppError(error).message);
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void loadList()
      .then((list) => list[0] && selectWorkflow(list[0].id))
      .catch((error) => setMessage(asAppError(error).message));
  }, []);

  const newWorkflow = () => {
    const reference = createReferenceWorkflowDraft();
    reference.name = '新建部署工作流';
    reference.description = '基于参考流程创建；保存后生成第一个不可变版本。';
    setDraft(reference);
    setValidation(null);
    setSelectedNodeId(reference.nodes[0]?.id ?? null);
    setZoom(1);
    setMessage('已创建未保存的参考工作流。');
  };

  const addNode = (type: WorkflowStepType) => {
    const node = nextNode(type, draft.nodes.length + 1);
    setDraft((current) => ({ ...current, nodes: [...current.nodes, node] }));
    setValidation(null);
    setSelectedNodeId(node.id);
  };

  const moveNode = (nodeId: string, position: { x: number; y: number }) => {
    setDraft((current) => ({
      ...current,
      nodes: current.nodes.map((node) => node.id === nodeId ? { ...node, position } : node),
    }));
    setValidation(null);
  };

  const changeDraft = (next: WorkflowDraft) => {
    setDraft(next);
    setValidation(null);
  };

  const deleteNode = (nodeId: string) => {
    const nodes = draft.nodes.filter((node) => node.id !== nodeId);
    changeDraft({
      ...draft,
      nodes,
      edges: draft.edges.filter((edge) => edge.from !== nodeId && edge.to !== nodeId),
    });
    setSelectedNodeId(nodes[0]?.id ?? null);
  };

  const validate = async () => {
    setBusy(true);
    setMessage('');
    try {
      const report = await api.validateWorkflow(draft);
      setValidation(report);
      setMessage(report.valid ? '工作流校验通过。' : `发现 ${report.diagnostics.length} 项问题。`);
    } catch (error) {
      setMessage(asAppError(error).message);
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    setBusy(true);
    setMessage('');
    try {
      const saved = await api.saveWorkflow(draft);
      setDraft(toDraft(saved));
      await loadList();
      setMessage(`已保存版本 v${saved.version}`);
    } catch (error) {
      setMessage(asAppError(error).message);
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!draft.id) return;
    setBusy(true);
    try {
      await api.deleteWorkflow(draft.id);
      const list = await loadList();
      if (list[0]) await selectWorkflow(list[0].id);
      else newWorkflow();
      setMessage('工作流已删除。');
    } catch (error) {
      setMessage(asAppError(error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="workflow-page" aria-labelledby="workflow-title">
      <header className="page-heading workflow-page__heading">
        <div>
          <span className="eyebrow">可恢复自动化 · Preview 可用</span>
          <h1 id="workflow-title">工作流编排</h1>
          <p>组合受控任务、日志和文件操作；画布只配置步骤，不提供 SSH 交互终端。</p>
        </div>
        <div className="workflow-heading-actions">
          <button type="button" className="secondary-button" onClick={newWorkflow} disabled={busy}>
            <Plus aria-hidden="true" />新建工作流
          </button>
          <button type="button" className="secondary-button" onClick={() => void validate()} disabled={busy}>
            <CheckCircle aria-hidden="true" />校验工作流
          </button>
          <button type="button" className="primary-button" onClick={save} disabled={busy}>
            <FloppyDisk aria-hidden="true" />保存工作流
          </button>
        </div>
      </header>

      {message && <div className="workflow-message" role="status">{message}</div>}

      <section className="silver-card workflow-records" aria-label="工作流列表">
        <header>
          <FlowArrow weight="duotone" aria-hidden="true" />
          <div><strong>已保存工作流</strong><small>每次改动保存为不可变版本</small></div>
        </header>
        <div className="workflow-records__list">
          {summaries.length === 0 && <span>尚无已保存工作流，可从参考流程开始。</span>}
          {summaries.map((summary) => (
            <button
              type="button"
              key={summary.id}
              className={draft.id === summary.id ? 'is-active' : ''}
              onClick={() => void selectWorkflow(summary.id)}
            >
              <span><strong>{summary.name}</strong><small>{summary.description}</small></span>
              <b>v{summary.currentVersion}</b>
            </button>
          ))}
        </div>
        <button type="button" className="workflow-delete" onClick={() => void remove()} disabled={busy || !draft.id}>
          <Trash aria-hidden="true" />删除工作流
        </button>
      </section>

      <div className="workflow-builder">
        <WorkflowLibrary onAdd={addNode} />
        <WorkflowCanvas
          draft={draft}
          selectedNodeId={selectedNodeId}
          zoom={zoom}
          onSelect={setSelectedNodeId}
          onMove={moveNode}
          onZoom={(value) => setZoom(Number(value.toFixed(1)))}
        />
        <div className="workflow-right-column">
          <WorkflowInspector
            draft={draft}
            node={draft.nodes.find((node) => node.id === selectedNodeId) ?? null}
            onChange={changeDraft}
            onDelete={deleteNode}
          />
          <WorkflowValidationPanel report={validation} onLocate={setSelectedNodeId} />
        </div>
      </div>
    </section>
  );
}
