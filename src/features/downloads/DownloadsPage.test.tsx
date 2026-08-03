import '@testing-library/jest-dom/vitest';
import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({ listExecutions: vi.fn(), getExecution: vi.fn() }));
vi.mock('../../api/tauri', () => ({ api: apiMocks }));

import { DownloadsPage } from './DownloadsPage';

describe('DownloadsPage', () => {
  beforeEach(() => {
    apiMocks.listExecutions.mockResolvedValue([{ id: 'execution-1' }, { id: 'execution-2' }]);
    apiMocks.getExecution.mockImplementation(async (id) => id === 'execution-1' ? { record: { id }, parameters: [], files: [{ id: 'file-1', relativePath: 'downloads/app.log', purpose: 'download', sizeBytes: 4096, sha256: 'b'.repeat(64) }] } : { record: { id }, parameters: [], files: [{ id: 'file-2', relativePath: 'logs/searches/result.txt', purpose: 'log_results_text', sizeBytes: 512, sha256: 'c'.repeat(64) }] });
  });

  it('lists only data-root-relative files returned by the backend', async () => {
    render(<DownloadsPage />);
    expect(await screen.findByText('downloads/app.log')).toBeVisible();
    expect(screen.getByText('logs/searches/result.txt')).toBeVisible();
    expect(screen.queryByText('C:\\private\\secret.txt')).not.toBeInTheDocument();
    expect(apiMocks.listExecutions).toHaveBeenCalledWith({});
    expect(apiMocks.getExecution).toHaveBeenCalledTimes(2);
  });
});
