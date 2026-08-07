import '@testing-library/jest-dom/vitest';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { ExecutionDetails } from '../../api/contracts';
import { ExecutionDrawer } from './ExecutionDrawer';

function details(status: ExecutionDetails['record']['status']): ExecutionDetails {
  return {
    record: {
      id: 'execution-1',
      serverId: 'server-1',
      taskId: 'system.summary',
      taskVersion: 1,
      category: 'system',
      status,
      createdAt: 1,
      startedAt: 1,
      finishedAt: 2,
      durationMs: 360,
      exitCode: status === 'succeeded' ? 0 : null,
      errorCategory: null,
      errorMessage: null,
      retryable: false,
      parametersSummary: null,
      outputSummary: null,
      remoteProcessGroup: null,
    },
    parameters: [],
    files: [],
  };
}

describe('ExecutionDrawer', () => {
  it('shows novice-friendly Chinese execution states', () => {
    const { rerender } = render(
      <ExecutionDrawer events={[]} details={details('succeeded')} running={false} onCancel={vi.fn()} />,
    );
    expect(screen.getByText('成功')).toBeVisible();
    expect(screen.queryByText('succeeded')).not.toBeInTheDocument();

    rerender(<ExecutionDrawer events={[]} details={null} running onCancel={vi.fn()} />);
    expect(screen.getByText('运行中')).toBeVisible();
    expect(screen.queryByText('running')).not.toBeInTheDocument();
  });
});
