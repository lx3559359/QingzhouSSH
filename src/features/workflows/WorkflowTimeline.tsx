import { CheckCircle, Circle, Clock, Question, WarningCircle, XCircle } from '@phosphor-icons/react';

import type { WorkflowDraft, WorkflowNodeStatus, WorkflowRunDetails } from '../../api/contracts';

const labels: Record<WorkflowNodeStatus, string> = {
  pending: '等待', running: '运行中', succeeded: '成功', failed: '失败', cancelled: '已取消',
  uncertain: '状态不确定', skipped: '已跳过',
};

function StatusIcon({ status }: { status: WorkflowNodeStatus }) {
  if (status === 'succeeded') return <CheckCircle weight="fill" aria-hidden="true" />;
  if (status === 'failed') return <XCircle weight="fill" aria-hidden="true" />;
  if (status === 'uncertain') return <Question weight="fill" aria-hidden="true" />;
  if (status === 'cancelled') return <WarningCircle weight="fill" aria-hidden="true" />;
  if (status === 'running') return <Clock weight="fill" aria-hidden="true" />;
  return <Circle weight="duotone" aria-hidden="true" />;
}

export function WorkflowTimeline({ details, draft }: { details: WorkflowRunDetails; draft: WorkflowDraft }) {
  const nodeById = new Map(draft.nodes.map((node) => [node.id, node]));
  const ordered = [...details.nodeRuns].sort((left, right) => {
    const leftIndex = draft.nodes.findIndex((node) => node.id === left.nodeId);
    const rightIndex = draft.nodes.findIndex((node) => node.id === right.nodeId);
    return leftIndex - rightIndex || left.attempt - right.attempt;
  });
  return (
    <ol className="workflow-run-timeline" aria-label="节点运行时间线">
      {ordered.length === 0 && <li className="workflow-run-timeline__empty">运行尚未产生节点记录。</li>}
      {ordered.map((nodeRun) => (
        <li key={`${nodeRun.nodeId}-${nodeRun.attempt}`} className={`workflow-run-node workflow-run-node--${nodeRun.status}`}>
          <StatusIcon status={nodeRun.status} />
          <span>
            <strong>{nodeById.get(nodeRun.nodeId)?.name ?? nodeRun.nodeId}</strong>
            <small>第 {nodeRun.attempt} 次尝试 · {labels[nodeRun.status]}</small>
            {nodeRun.errorMessage && <em>{nodeRun.errorMessage}</em>}
          </span>
          {nodeRun.executionId && <code>{nodeRun.executionId}</code>}
        </li>
      ))}
    </ol>
  );
}
