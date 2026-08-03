import { describe, expect, it } from 'vitest';

import { previewApi } from './preview';

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
