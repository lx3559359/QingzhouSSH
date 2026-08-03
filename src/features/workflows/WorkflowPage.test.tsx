import '@testing-library/jest-dom/vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  listWorkflows: vi.fn(),
  getWorkflow: vi.fn(),
  saveWorkflow: vi.fn(),
  deleteWorkflow: vi.fn(),
}));

vi.mock('../../api/tauri', () => ({ api: apiMocks }));

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
  currentVersion: definition.version,
  createdAt: 1,
  updatedAt: 1,
};

describe('WorkflowPage canvas', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'PointerEvent', { configurable: true, value: MouseEvent });
    vi.clearAllMocks();
    apiMocks.listWorkflows.mockResolvedValue([summary]);
    apiMocks.getWorkflow.mockResolvedValue(definition);
    apiMocks.saveWorkflow.mockResolvedValue({ ...definition, version: 2 });
    apiMocks.deleteWorkflow.mockResolvedValue(true);
  });

  it('shows workflow records, eight step types, selectable nodes and SVG connections', async () => {
    const user = userEvent.setup();
    render(<WorkflowPage />);

    expect(await screen.findByRole('button', { name: /参考部署流程/ })).toBeVisible();
    for (const label of ['开始', '快捷任务', '自定义命令', '上传文件', '下载文件', '检索日志', '条件判断', '停止并提示']) {
      expect(screen.getByRole('button', { name: `添加${label}步骤` })).toBeVisible();
    }
    expect(screen.getByLabelText('工作流连接')).toBeVisible();

    await user.click(screen.getByRole('button', { name: /检查系统概况/ }));
    expect(screen.getByRole('button', { name: /检查系统概况/ })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.queryByText('工作流将在下一里程碑开放')).not.toBeInTheDocument();
  });

  it('creates, saves, deletes, adds and drags nodes, and changes canvas zoom', async () => {
    const user = userEvent.setup();
    render(<WorkflowPage />);
    await screen.findByRole('button', { name: /参考部署流程/ });

    await user.click(screen.getByRole('button', { name: '新建工作流' }));
    await user.click(screen.getByRole('button', { name: '添加上传文件步骤' }));
    const added = screen.getByRole('button', { name: /上传文件 6/ });
    expect(added).toBeVisible();

    fireEvent.pointerDown(added, { clientX: 10, clientY: 10, pointerId: 1 });
    fireEvent.pointerMove(added, { clientX: 50, clientY: 35, pointerId: 1 });
    fireEvent.pointerUp(added, { pointerId: 1 });
    expect(added).toHaveStyle({ left: '580px', top: '365px' });

    await user.click(screen.getByRole('button', { name: '放大画布' }));
    expect(screen.getByText('110%')).toBeVisible();
    await user.click(screen.getByRole('button', { name: '保存工作流' }));
    expect(apiMocks.saveWorkflow).toHaveBeenCalledWith(expect.objectContaining({ nodes: expect.any(Array) }));

    await user.click(screen.getByRole('button', { name: /参考部署流程/ }));
    await user.click(screen.getByRole('button', { name: '删除工作流' }));
    await waitFor(() => expect(apiMocks.deleteWorkflow).toHaveBeenCalledWith('workflow-1'));
  });
});
