import type { ExecutionStatus } from '../api/contracts';

const executionStatusLabels: Record<ExecutionStatus, string> = {
  queued: '排队中',
  running: '运行中',
  succeeded: '成功',
  failed: '失败',
  cancelled: '已取消',
  uncertain: '状态待确认',
};

const executionCategoryLabels: Record<string, string> = {
  system: '系统检查',
  service: '服务管理',
  logs: '日志检索',
  transfer: '文件传输',
  advanced: '高级操作',
};

export function executionStatusLabel(status: ExecutionStatus | 'idle') {
  return status === 'idle' ? '等待运行' : executionStatusLabels[status];
}

export function executionCategoryLabel(category: string) {
  return executionCategoryLabels[category] ?? category;
}
