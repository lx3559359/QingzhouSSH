import { beforeEach, describe, expect, it, vi } from 'vitest';

const { open } = vi.hoisted(() => ({ open: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open }));

import { chooseDirectory } from './dialogs';

describe('directory chooser', () => {
  beforeEach(() => {
    open.mockReset();
    window.history.replaceState({}, '', '/?preview=ready');
  });

  it('uses the supplied project-local path in browser preview without invoking Tauri', async () => {
    await expect(chooseDirectory({
      title: '选择测试目录',
      previewPath: '.local\\dev-data\\preview-target',
    })).resolves.toBe('.local\\dev-data\\preview-target');
    expect(open).not.toHaveBeenCalled();
  });
});
