import '@testing-library/jest-dom/vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  listWorkflows: vi.fn(),
  getWorkflow: vi.fn(),
  saveWorkflow: vi.fn(),
  deleteWorkflow: vi.fn(),
  validateWorkflow: vi.fn(),
}));

vi.mock('../../api/tauri', () => ({ api: apiMocks, asAppError: (error: Error) => ({ code: 'test', message: error.message }) }));

import type { WorkflowDefinition, WorkflowSummary } from '../../api/contracts';
import { createReferenceWorkflowDraft } from './fixtures';
import { WorkflowPage } from './WorkflowPage';

const definition: WorkflowDefinition = {
  ...createReferenceWorkflowDraft(),
  id: 'workflow-1',
  version: 1,
  checksumSha256: 'a'.repeat(64),
};

const summary: WorkflowSummary = {
  id: definition.id,
  name: definition.name,
  description: definition.description,
  currentVersion: 1,
  createdAt: 1,
  updatedAt: 1,
};

describe('WorkflowInspector', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'PointerEvent', { configurable: true, value: MouseEvent });
    vi.clearAllMocks();
    window.localStorage.clear();
    window.sessionStorage.clear();
    apiMocks.listWorkflows.mockResolvedValue([summary]);
    apiMocks.getWorkflow.mockResolvedValue(definition);
    apiMocks.saveWorkflow.mockResolvedValue(definition);
    apiMocks.deleteWorkflow.mockResolvedValue(true);
    apiMocks.validateWorkflow.mockResolvedValue({ valid: true, startNodeId: definition.nodes[0].id, diagnostics: [] });
  });

  it('edits task parameters and explicit next/true/false connections, then deletes a node', async () => {
    const user = userEvent.setup();
    render(<WorkflowPage />);
    await screen.findByRole('button', { name: /参考部署流程/ });

    await user.click(screen.getByRole('button', { name: /检查系统概况/ }));
    expect(screen.getByLabelText('任务 ID')).toHaveValue('system.overview');
    expect(screen.getByLabelText('下一步')).toHaveValue(definition.nodes[2].id);
    await user.clear(screen.getByLabelText('任务 ID'));
    await user.type(screen.getByLabelText('任务 ID'), 'service.restart');

    await user.click(screen.getByRole('button', { name: /健康检查通过/ }));
    expect(screen.getByLabelText('真分支')).toHaveValue(definition.nodes[3].id);
    expect(screen.getByLabelText('假分支')).toHaveValue(definition.nodes[4].id);
    await user.selectOptions(screen.getByLabelText('假分支'), '');
    expect(screen.getByLabelText('假分支')).toHaveValue('');

    await user.click(screen.getByRole('button', { name: '删除当前节点' }));
    expect(screen.queryByRole('button', { name: /健康检查通过/ })).not.toBeInTheDocument();
  });

  it('locates graph diagnostics returned by the shared validator', async () => {
    const user = userEvent.setup();
    apiMocks.validateWorkflow.mockResolvedValue({
      valid: false,
      startNodeId: definition.nodes[0].id,
      diagnostics: [
        { code: 'cycle', nodeId: definition.nodes[2].id, message: '工作流不能包含环。' },
        { code: 'unreachable_node', nodeId: definition.nodes[4].id, message: '停止节点不可到达。' },
      ],
    });
    render(<WorkflowPage />);
    await screen.findByRole('button', { name: /参考部署流程/ });

    await user.click(screen.getByRole('button', { name: '校验工作流' }));
    expect(await screen.findByText('工作流不能包含环。')).toBeVisible();
    expect(screen.getByText('停止节点不可到达。')).toBeVisible();
    await user.click(screen.getByRole('button', { name: /定位：工作流不能包含环/ }));
    expect(screen.getByRole('button', { name: /健康检查通过/ })).toHaveAttribute('aria-pressed', 'true');
  });

  it('keeps script text out of browser storage and exposes only a length summary in the risk area', async () => {
    const user = userEvent.setup();
    const localSpy = vi.spyOn(Storage.prototype, 'setItem');
    render(<WorkflowPage />);
    await screen.findByRole('button', { name: /参考部署流程/ });

    await user.click(screen.getByRole('button', { name: '添加自定义命令步骤' }));
    await user.selectOptions(screen.getByLabelText('执行方式'), 'script');
    const canary = 'SECRET_SCRIPT_CANARY\necho safe';
    await user.type(screen.getByLabelText('脚本内容'), canary);

    const summaryArea = screen.getByLabelText('危险节点摘要');
    expect(within(summaryArea).getByText(/脚本 · 2 行 · 30 字符/)).toBeVisible();
    expect(within(summaryArea).queryByText(canary)).not.toBeInTheDocument();
    expect(localSpy).not.toHaveBeenCalled();
    expect(window.localStorage.length).toBe(0);
    expect(window.sessionStorage.length).toBe(0);
    localSpy.mockRestore();
  });
});
