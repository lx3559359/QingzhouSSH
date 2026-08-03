import { describe, expect, it } from 'vitest';

import { previewApi, resetWorkflowPreviewForTests } from './preview';
import { createReferenceWorkflowDraft } from '../features/workflows/fixtures';

describe('preview data root', () => {
  it('uses the project-local development data root', async () => {
    const status = await previewApi.bootstrapStatus();

    expect(status).toEqual({
      state: 'ready',
      dataRoot: import.meta.env.VITE_QINGZHOU_DATA_ROOT,
    });
    expect(status.dataRoot).toContain('轻量化SSH快捷工具');
    expect(status.dataRoot).toMatch(/[\\/].local[\\/]dev-data$/);
    expect(status.dataRoot).not.toBe('D:\\QingzhouSSH\\data');
  });
});

describe('workflow preview API', () => {
  it('saves immutable-looking versions and reports validation diagnostics', async () => {
    resetWorkflowPreviewForTests();
    const draft = createReferenceWorkflowDraft();
    const first = await previewApi.saveWorkflow(draft);
    const unchanged = await previewApi.saveWorkflow({ ...first });
    const changed = await previewApi.saveWorkflow({ ...first, description: 'changed' });

    expect(first.version).toBe(1);
    expect(unchanged.version).toBe(1);
    expect(changed.version).toBe(2);
    expect(await previewApi.getWorkflow(first.id, 1)).toEqual(first);
    expect((await previewApi.validateWorkflow({ ...draft, nodes: [] })).valid).toBe(false);
  });

  it('models successful and false-branch runs with real workflow DTO shapes', async () => {
    resetWorkflowPreviewForTests();
    const definition = await previewApi.saveWorkflow(createReferenceWorkflowDraft());
    const emitted: number[] = [];
    const details = await previewApi.startWorkflowRun(
      {
        workflowId: definition.id,
        workflowVersion: definition.version,
        serverId: 'preview-openeuler',
        dangerousConfirmed: true,
      },
      (event) => emitted.push(event.sequence),
    );

    expect(details.run.status).toBe('succeeded');
    expect(details.nodeRuns.some((node) => node.status === 'skipped')).toBe(true);
    expect(emitted).toEqual([...emitted].sort((left, right) => left - right));
    expect(details.events.map((event) => event.sequence)).toEqual(emitted);
  });

  it('models pause, retry, cancellation, rollback, cleanup and diagnostics without disk writes', async () => {
    resetWorkflowPreviewForTests();
    const failure = createReferenceWorkflowDraft();
    failure.name = '失败暂停演示';
    const task = failure.nodes.find((node) => node.config.type === 'task');
    if (task?.config.type === 'task') task.config.taskId = 'preview.fail';
    const failedDefinition = await previewApi.saveWorkflow(failure);
    const failed = await previewApi.startWorkflowRun(
      {
        workflowId: failedDefinition.id,
        workflowVersion: null,
        serverId: 'preview-openeuler',
        dangerousConfirmed: true,
      },
      () => undefined,
    );
    expect(failed.run.status).toBe('paused');
    expect((await previewApi.retryWorkflowNode(failed.run.id, true, () => undefined)).run.status).toBe(
      'succeeded',
    );

    const cancellable = createReferenceWorkflowDraft();
    cancellable.id = null;
    cancellable.name = '取消演示';
    const cancelDefinition = await previewApi.saveWorkflow(cancellable);
    const running = await previewApi.startWorkflowRun(
      {
        workflowId: cancelDefinition.id,
        workflowVersion: null,
        serverId: 'preview-openeuler',
        dangerousConfirmed: true,
      },
      () => undefined,
    );
    expect(running.run.status).toBe('running');
    await previewApi.cancelWorkflowRun(running.run.id);
    expect((await previewApi.getWorkflowRun(running.run.id))?.run.status).toBe('cancelled');

    const rolledBack = await previewApi.rollbackWorkflowRun(failed.run.id, true);
    expect(rolledBack.run.status).toBe('rolled_back');
    expect(await previewApi.cleanupWorkflowRestorePoints(failed.run.id)).toBeGreaterThanOrEqual(0);
    const diagnostics = await previewApi.exportWorkflowDiagnostics(failed.run.id);
    expect(diagnostics.relativePath).toMatch(/^downloads\//);
    expect(diagnostics.purpose).toBe('workflow_diagnostics_preview');
  });
});
