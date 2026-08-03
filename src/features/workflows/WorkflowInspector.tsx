import { ShieldWarning, Trash } from '@phosphor-icons/react';

import type {
  WorkflowDraft,
  WorkflowEdgeBranch,
  WorkflowNode,
  WorkflowNodeConfig,
} from '../../api/contracts';

export interface WorkflowRiskSummary {
  nodeId: string;
  title: string;
  detail: string;
}

export function summarizeWorkflowRisks(draft: WorkflowDraft): WorkflowRiskSummary[] {
  return draft.nodes.flatMap((node) => {
    if (node.config.type === 'custom') {
      const lines = node.config.content.length === 0 ? 0 : node.config.content.split(/\r?\n/).length;
      return [{
        nodeId: node.id,
        title: node.name,
        detail: `${node.config.mode === 'script' ? '脚本' : '命令'} · ${lines} 行 · ${node.config.content.length} 字符`,
      }];
    }
    if (node.config.type === 'upload' && node.config.overwrite) {
      return [{ nodeId: node.id, title: node.name, detail: `覆盖远端文件 · 恢复点${node.config.createRestorePoint ? '已启用' : '未启用'}` }];
    }
    if (node.config.type === 'task' && /(restart|stop|delete|remove)/i.test(node.config.taskId)) {
      return [{ nodeId: node.id, title: node.name, detail: `可能改变服务状态 · ${node.config.taskId}` }];
    }
    return [];
  });
}

function ConnectionSelect({
  label,
  branch,
  draft,
  node,
  onChange,
}: {
  label: string;
  branch: WorkflowEdgeBranch;
  draft: WorkflowDraft;
  node: WorkflowNode;
  onChange: (branch: WorkflowEdgeBranch, target: string) => void;
}) {
  const current = draft.edges.find((edge) => edge.from === node.id && edge.branch === branch)?.to ?? '';
  return (
    <label>
      {label}
      <select value={current} onChange={(event) => onChange(branch, event.target.value)}>
        <option value="">无连接</option>
        {draft.nodes.filter((candidate) => candidate.id !== node.id).map((candidate) => (
          <option key={candidate.id} value={candidate.id}>{candidate.name}</option>
        ))}
      </select>
    </label>
  );
}

export function WorkflowInspector({
  draft,
  node,
  onChange,
  onDelete,
}: {
  draft: WorkflowDraft;
  node: WorkflowNode | null;
  onChange: (draft: WorkflowDraft) => void;
  onDelete: (nodeId: string) => void;
}) {
  const risks = summarizeWorkflowRisks(draft);
  if (!node) {
    return (
      <aside className="silver-card workflow-inspector" aria-label="节点参数">
        <span className="workflow-panel-kicker">节点检查器</span>
        <p>从画布选择一个节点以编辑参数与连接。</p>
      </aside>
    );
  }

  const updateNode = (updated: WorkflowNode) => {
    onChange({ ...draft, nodes: draft.nodes.map((candidate) => candidate.id === node.id ? updated : candidate) });
  };
  const updateConfig = (config: WorkflowNodeConfig) => updateNode({ ...node, config });
  const updateConnection = (branch: WorkflowEdgeBranch, target: string) => {
    const edges = draft.edges.filter((edge) => !(edge.from === node.id && edge.branch === branch));
    onChange({ ...draft, edges: target ? [...edges, { from: node.id, to: target, branch }] : edges });
  };

  return (
    <aside className="silver-card workflow-inspector" aria-label="节点参数">
      <header>
        <span className="workflow-panel-kicker">节点检查器</span>
        <strong>{node.name}</strong>
        <small>{node.config.type}</small>
      </header>
      <div className="workflow-inspector__form">
        <label>
          节点名称
          <input value={node.name} onChange={(event) => updateNode({ ...node, name: event.target.value })} />
        </label>
        <NodeFields node={node} draft={draft} updateConfig={updateConfig} />
        {node.config.type === 'condition' ? (
          <fieldset>
            <legend>分支连接</legend>
            <ConnectionSelect label="真分支" branch="true" draft={draft} node={node} onChange={updateConnection} />
            <ConnectionSelect label="假分支" branch="false" draft={draft} node={node} onChange={updateConnection} />
          </fieldset>
        ) : node.config.type !== 'stop' ? (
          <fieldset>
            <legend>流程连接</legend>
            <ConnectionSelect label="下一步" branch="success" draft={draft} node={node} onChange={updateConnection} />
          </fieldset>
        ) : null}
      </div>

      <section className="workflow-risk-summary" aria-label="危险节点摘要">
        <header><ShieldWarning aria-hidden="true" /><strong>危险节点摘要</strong></header>
        {risks.length === 0 ? <p>当前未识别到需要二次确认的节点。</p> : (
          <ul>{risks.map((risk) => <li key={risk.nodeId}><strong>{risk.title}</strong><span>{risk.detail}</span></li>)}</ul>
        )}
      </section>
      <button type="button" className="workflow-node-delete" onClick={() => onDelete(node.id)}>
        <Trash aria-hidden="true" />删除当前节点
      </button>
    </aside>
  );
}

function NodeFields({
  node,
  draft,
  updateConfig,
}: {
  node: WorkflowNode;
  draft: WorkflowDraft;
  updateConfig: (config: WorkflowNodeConfig) => void;
}) {
  const config = node.config;
  switch (config.type) {
    case 'start':
      return <p className="workflow-field-note">开始节点没有执行参数，一个工作流只能有一个。</p>;
    case 'task':
      return (
        <>
          <label>任务 ID<input value={config.taskId} onChange={(event) => updateConfig({ ...config, taskId: event.target.value })} /></label>
          <label>任务版本<input type="number" min="1" value={config.taskVersion} onChange={(event) => updateConfig({ ...config, taskVersion: Number(event.target.value) })} /></label>
          <label>参数 JSON<textarea value={JSON.stringify(config.parameters, null, 2)} onChange={(event) => {
            try {
              const parameters = JSON.parse(event.target.value) as Record<string, unknown>;
              updateConfig({ ...config, parameters });
            } catch { /* Keep the last valid typed parameters. */ }
          }} /></label>
        </>
      );
    case 'custom':
      return (
        <>
          <label>执行方式<select value={config.mode} onChange={(event) => updateConfig({ ...config, mode: event.target.value as 'command' | 'script' })}>
            <option value="command">单条命令</option><option value="script">多行脚本</option>
          </select></label>
          <label>{config.mode === 'script' ? '脚本内容' : '命令内容'}<textarea value={config.content} onChange={(event) => updateConfig({ ...config, content: event.target.value })} /></label>
          <label>超时（秒）<input type="number" min="1" max="3600" value={config.timeoutSeconds} onChange={(event) => updateConfig({ ...config, timeoutSeconds: Number(event.target.value) })} /></label>
          <p className="workflow-field-note">正文仅保存在工作流版本中，不进入 localStorage、sessionStorage 或确认摘要。</p>
        </>
      );
    case 'upload':
      return (
        <>
          <label>本地文件<input value={config.localPath} onChange={(event) => updateConfig({ ...config, localPath: event.target.value })} /></label>
          <label>远端绝对路径<input value={config.remotePath} onChange={(event) => updateConfig({ ...config, remotePath: event.target.value })} /></label>
          <label className="workflow-check"><input type="checkbox" checked={config.overwrite} onChange={(event) => updateConfig({ ...config, overwrite: event.target.checked })} />覆盖已有文件</label>
          <label className="workflow-check"><input type="checkbox" checked={config.createRestorePoint} onChange={(event) => updateConfig({ ...config, createRestorePoint: event.target.checked })} />覆盖前创建恢复点</label>
        </>
      );
    case 'download':
      return (
        <>
          <label>远端绝对路径<input value={config.remotePath} onChange={(event) => updateConfig({ ...config, remotePath: event.target.value })} /></label>
          <label>建议文件名<input value={config.suggestedName} onChange={(event) => updateConfig({ ...config, suggestedName: event.target.value })} /></label>
          <label className="workflow-check"><input type="checkbox" checked={config.overwrite} onChange={(event) => updateConfig({ ...config, overwrite: event.target.checked })} />覆盖同名下载</label>
        </>
      );
    case 'logSearch':
      return (
        <>
          <label>日志路径<input value={config.path} onChange={(event) => updateConfig({ ...config, path: event.target.value })} /></label>
          <label>关键词<input value={config.keyword} onChange={(event) => updateConfig({ ...config, keyword: event.target.value })} /></label>
          <label>上下文行数<input type="number" min="0" max="20" value={config.contextLines} onChange={(event) => updateConfig({ ...config, contextLines: Number(event.target.value) })} /></label>
          <label>最大匹配数<input type="number" min="1" max="10000" value={config.limit} onChange={(event) => updateConfig({ ...config, limit: Number(event.target.value) })} /></label>
          <label className="workflow-check"><input type="checkbox" checked={config.caseSensitive} onChange={(event) => updateConfig({ ...config, caseSensitive: event.target.checked })} />区分大小写</label>
        </>
      );
    case 'condition': {
      const predicate = config.predicate;
      const setKind = (kind: string) => {
        if (kind === 'resultField') updateConfig({ ...config, predicate: { kind: 'resultField', path: 'status', operator: 'equal', value: 'ok' } });
        else if (kind === 'outputContains') updateConfig({ ...config, predicate: { kind: 'outputContains', text: '', negated: false } });
        else updateConfig({ ...config, predicate: { kind: 'exitCode', operator: 'equal', value: 0 } });
      };
      return (
        <>
          <label>来源节点<select value={config.sourceNodeId} onChange={(event) => updateConfig({ ...config, sourceNodeId: event.target.value })}>
            <option value="">请选择已完成节点</option>
            {draft.nodes.filter((candidate) => candidate.id !== node.id && candidate.config.type !== 'condition').map((candidate) => (
              <option key={candidate.id} value={candidate.id}>{candidate.name}</option>
            ))}
          </select></label>
          <label>条件类型<select value={predicate.kind} onChange={(event) => setKind(event.target.value)}>
            <option value="exitCode">退出码</option><option value="resultField">结果字段</option><option value="outputContains">输出包含</option>
          </select></label>
          {predicate.kind === 'exitCode' && <label>比较值<input type="number" value={predicate.value} onChange={(event) => updateConfig({ ...config, predicate: { ...predicate, value: Number(event.target.value) } })} /></label>}
          {predicate.kind === 'resultField' && <>
            <label>字段路径<input value={predicate.path} onChange={(event) => updateConfig({ ...config, predicate: { ...predicate, path: event.target.value } })} /></label>
            <label>比较值<input value={String(predicate.value)} onChange={(event) => updateConfig({ ...config, predicate: { ...predicate, value: event.target.value } })} /></label>
          </>}
          {predicate.kind === 'outputContains' && <>
            <label>固定文本<input value={predicate.text} maxLength={512} onChange={(event) => updateConfig({ ...config, predicate: { ...predicate, text: event.target.value } })} /></label>
            <label className="workflow-check"><input type="checkbox" checked={predicate.negated} onChange={(event) => updateConfig({ ...config, predicate: { ...predicate, negated: event.target.checked } })} />改为“不包含”</label>
          </>}
        </>
      );
    }
    case 'stop':
      return <label>停止提示<textarea value={config.message} onChange={(event) => updateConfig({ ...config, message: event.target.value })} /></label>;
  }
}
