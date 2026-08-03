import '@testing-library/jest-dom/vitest';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  listWorkflows: vi.fn(), getWorkflow: vi.fn(), saveWorkflow: vi.fn(), deleteWorkflow: vi.fn(),
  validateWorkflow: vi.fn(), listServers: vi.fn(), listWorkflowRuns: vi.fn(), getWorkflowRun: vi.fn(),
  startWorkflowRun: vi.fn(), retryWorkflowNode: vi.fn(), cancelWorkflowRun: vi.fn(),
  rollbackWorkflowRun: vi.fn(), cleanupWorkflowRestorePoints: vi.fn(), exportWorkflowDiagnostics: vi.fn(),
}));

vi.mock('../../api/tauri', () => ({ api: apiMocks, asAppError: (error: Error) => ({ code: 'test', message: error.message }) }));

import type { WorkflowDefinition, WorkflowRunDetails, WorkflowRunStatus, WorkflowSummary } from '../../api/contracts';
import { createReferenceWorkflowDraft } from './fixtures';
import { WorkflowPage } from './WorkflowPage';

const definition: WorkflowDefinition = {
  ...createReferenceWorkflowDraft(), id: 'workflow-1', version: 1, checksumSha256: 'a'.repeat(64),
};
const summary: WorkflowSummary = {
  id: definition.id, name: definition.name, description: definition.description,
  currentVersion: 1, createdAt: 1, updatedAt: 1,
};
const server = {
  id: 'server-1', name: 'openEuler 生产机', host: '10.0.0.8', port: 22, username: 'ops',
  authKind: 'password' as const, credentialId: 'credential-1',
};

function runDetails(status: WorkflowRunStatus): WorkflowRunDetails {
  const running = status === 'running';
  return {
    run: {
      id: 'run-1', workflowId: definition.id, workflowVersion: 1, serverId: server.id, status,
      currentNodeId: running || status === 'paused' ? definition.nodes[1].id : null,
      createdAt: 1, startedAt: 1, finishedAt: running ? null : 2, durationMs: running ? null : 1,
      errorCategory: status === 'paused' ? 'execution_failed' : null,
      errorMessage: status === 'paused' ? '任务执行失败，等待重试。' : status === 'uncertain' ? '无法确认远端进程状态。' : null,
      retryable: status === 'paused',
    },
    nodeRuns: [{
      runId: 'run-1', nodeId: definition.nodes[1].id, attempt: 1,
      status: status === 'paused' ? 'failed' : running ? 'running' : status === 'uncertain' ? 'uncertain' : 'succeeded',
      executionId: 'execution-1', startedAt: 1, finishedAt: running ? null : 2, durationMs: running ? null : 1,
      exitCode: status === 'succeeded' ? 0 : null, result: null, outputSummary: null,
      errorMessage: status === 'paused' ? '模拟失败' : null, retryable: status === 'paused',
    }],
    restorePoints: [{
      id: 'restore-1', runId: 'run-1', nodeId: definition.nodes[1].id,
      remotePath: '/opt/app/config.yml', relativePath: 'backups/workflows/run-1/config.yml',
      originalExisted: true, sizeBytes: 24, sha256: 'b'.repeat(64), status: 'available',
      applicability: { serverId: server.id }, errorMessage: null, createdAt: 1, updatedAt: 1,
    }],
    events: [],
  };
}

describe('WorkflowRunPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.listWorkflows.mockResolvedValue([summary]);
    apiMocks.getWorkflow.mockResolvedValue(definition);
    apiMocks.saveWorkflow.mockResolvedValue(definition);
    apiMocks.deleteWorkflow.mockResolvedValue(true);
    apiMocks.validateWorkflow.mockResolvedValue({ valid: true, startNodeId: definition.nodes[0].id, diagnostics: [] });
    apiMocks.listServers.mockResolvedValue([server]);
    apiMocks.listWorkflowRuns.mockResolvedValue([]);
    apiMocks.getWorkflowRun.mockResolvedValue(null);
    apiMocks.startWorkflowRun.mockImplementation(async (_request, onEvent) => {
      onEvent({ type: 'nodeStarted', sequence: 1, emittedAt: 1, runId: 'run-1', nodeId: definition.nodes[1].id, attempt: 1 });
      return runDetails('succeeded');
    });
    apiMocks.retryWorkflowNode.mockResolvedValue(runDetails('succeeded'));
    apiMocks.cancelWorkflowRun.mockResolvedValue(undefined);
    apiMocks.rollbackWorkflowRun.mockResolvedValue(runDetails('rolled_back'));
    apiMocks.cleanupWorkflowRestorePoints.mockResolvedValue(1);
    apiMocks.exportWorkflowDiagnostics.mockResolvedValue({
      id: 'diag-1', relativePath: 'downloads/run-1-diagnostics.json', purpose: 'workflow_diagnostics',
      sizeBytes: 128, sha256: 'c'.repeat(64),
    });
  });

  it('preflights a saved workflow, targets a server and renders the returned node timeline', async () => {
    const user = userEvent.setup();
    render(<WorkflowPage />);
    expect(await screen.findByLabelText('目标服务器')).toHaveValue(server.id);

    await user.click(screen.getByRole('button', { name: '运行工作流' }));
    await waitFor(() => expect(apiMocks.validateWorkflow).toHaveBeenCalled());
    expect(apiMocks.startWorkflowRun).toHaveBeenCalledWith(
      { workflowId: 'workflow-1', workflowVersion: 1, serverId: 'server-1', dangerousConfirmed: false },
      expect.any(Function),
    );
    expect(await screen.findByText('运行成功')).toBeVisible();
    expect(within(screen.getByLabelText('节点运行时间线')).getByText('检查系统概况')).toBeVisible();
  });

  it('confirms only a script summary before starting a dangerous workflow', async () => {
    const user = userEvent.setup();
    render(<WorkflowPage />);
    await screen.findByLabelText('目标服务器');
    await user.click(screen.getByRole('button', { name: '添加自定义命令步骤' }));
    await user.selectOptions(screen.getByLabelText('执行方式'), 'script');
    const canary = 'SECRET_SCRIPT_CANARY\necho safe';
    await user.type(screen.getByLabelText('脚本内容'), canary);
    apiMocks.saveWorkflow.mockImplementation(async (draft) => ({
      ...draft, id: 'workflow-1', version: 2, checksumSha256: 'd'.repeat(64),
    }));
    await user.click(screen.getByRole('button', { name: '保存工作流' }));
    await screen.findByText('已保存版本 v2');
    await user.click(screen.getByRole('button', { name: '运行工作流' }));

    const dialog = screen.getByRole('dialog', { name: '确认工作流危险操作' });
    expect(within(dialog).getByText(/脚本 · 2 行 · 30 字符/)).toBeVisible();
    expect(within(dialog).queryByText(canary)).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: '确认并运行' }));
    expect(apiMocks.startWorkflowRun).toHaveBeenCalledWith(
      expect.objectContaining({ dangerousConfirmed: true }), expect.any(Function),
    );
  });

  it('restores paused history, retries the failed node, and cancels a running workflow', async () => {
    const user = userEvent.setup();
    apiMocks.listWorkflowRuns.mockResolvedValue([runDetails('paused').run]);
    apiMocks.getWorkflowRun.mockResolvedValue(runDetails('paused'));
    render(<WorkflowPage />);
    expect(await screen.findByText('任务执行失败，等待重试。')).toBeVisible();
    await user.click(screen.getByRole('button', { name: '重试失败节点' }));
    expect(apiMocks.retryWorkflowNode).toHaveBeenCalledWith('run-1', false, expect.any(Function));
    expect(await screen.findByText('运行成功')).toBeVisible();

    apiMocks.startWorkflowRun.mockResolvedValue(runDetails('running'));
    await user.click(screen.getByRole('button', { name: '运行工作流' }));
    expect(await screen.findByRole('button', { name: '取消运行' })).toBeVisible();
    apiMocks.getWorkflowRun.mockResolvedValue(runDetails('cancelled'));
    await user.click(screen.getByRole('button', { name: '取消运行' }));
    expect(apiMocks.cancelWorkflowRun).toHaveBeenCalledWith('run-1');
    expect(await screen.findByText('已取消')).toBeVisible();
  });

  it('explains uncertain state and confirms rollback, cleanup and diagnostics', async () => {
    const user = userEvent.setup();
    apiMocks.listWorkflowRuns.mockResolvedValue([runDetails('uncertain').run]);
    apiMocks.getWorkflowRun.mockResolvedValue(runDetails('uncertain'));
    render(<WorkflowPage />);
    expect(await screen.findByText('无法确认远端进程状态。')).toBeVisible();

    await user.click(screen.getByRole('button', { name: '回滚恢复点' }));
    const dialog = screen.getByRole('dialog', { name: '确认回滚工作流' });
    expect(within(dialog).getByText('/opt/app/config.yml')).toBeVisible();
    await user.click(within(dialog).getByRole('button', { name: '确认回滚' }));
    expect(apiMocks.rollbackWorkflowRun).toHaveBeenCalledWith('run-1', true);

    await user.click(screen.getByRole('button', { name: '清理恢复点' }));
    expect(apiMocks.cleanupWorkflowRestorePoints).toHaveBeenCalledWith('run-1');
    await user.click(screen.getByRole('button', { name: '导出诊断' }));
    expect(await screen.findByText('downloads/run-1-diagnostics.json')).toBeVisible();
  });
});
