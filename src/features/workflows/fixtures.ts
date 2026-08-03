import type { WorkflowDraft } from '../../api/contracts';

export function createReferenceWorkflowDraft(): WorkflowDraft {
  const start = '10000000-0000-4000-8000-000000000001';
  const task = '10000000-0000-4000-8000-000000000002';
  const condition = '10000000-0000-4000-8000-000000000003';
  const healthy = '10000000-0000-4000-8000-000000000004';
  const stopped = '10000000-0000-4000-8000-000000000005';
  return {
    id: null,
    name: '参考部署流程（Preview）',
    description: '浏览器内存演示：检查系统后按条件分支，不连接真实服务器。',
    nodes: [
      {
        id: start,
        name: '开始',
        position: { x: 60, y: 130 },
        config: { type: 'start' },
      },
      {
        id: task,
        name: '检查系统概况',
        position: { x: 300, y: 130 },
        config: {
          type: 'task',
          taskId: 'system.overview',
          taskVersion: 1,
          parameters: { previewCondition: false },
        },
      },
      {
        id: condition,
        name: '健康检查通过？',
        position: { x: 540, y: 130 },
        config: {
          type: 'condition',
          sourceNodeId: task,
          predicate: { kind: 'exitCode', operator: 'equal', value: 0 },
        },
      },
      {
        id: healthy,
        name: '部署完成',
        position: { x: 780, y: 60 },
        config: { type: 'stop', message: '部署完成' },
      },
      {
        id: stopped,
        name: '停止并提示',
        position: { x: 780, y: 230 },
        config: { type: 'stop', message: '健康检查未通过' },
      },
    ],
    edges: [
      { from: start, to: task, branch: 'success' },
      { from: task, to: condition, branch: 'success' },
      { from: condition, to: healthy, branch: 'true' },
      { from: condition, to: stopped, branch: 'false' },
    ],
  };
}
