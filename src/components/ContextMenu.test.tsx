import '@testing-library/jest-dom/vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ContextMenu } from './ContextMenu';

describe('ContextMenu', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('focuses the first enabled action, explains disabled actions and closes after selection', async () => {
    const user = userEvent.setup();
    const copy = vi.fn();
    const close = vi.fn();
    render(
      <ContextMenu
        position={{ x: 30, y: 40 }}
        onClose={close}
        items={[
          { id: 'download', label: '下载', disabled: true, disabledReason: '当前对象不是文件', onSelect: vi.fn() },
          { id: 'copy-path', label: '复制完整路径', onSelect: copy },
        ]}
      />,
    );

    expect(screen.getByRole('menu')).toBeVisible();
    expect(screen.getByRole('menuitem', { name: '复制完整路径' })).toHaveFocus();
    expect(screen.getByRole('menuitem', { name: /下载.*当前对象不是文件/ })).toBeDisabled();

    await user.click(screen.getByRole('menuitem', { name: '复制完整路径' }));
    expect(copy).toHaveBeenCalledTimes(1);
    expect(close).toHaveBeenCalledTimes(1);
  });

  it('closes on Escape and an outside pointer press', () => {
    const escapeClose = vi.fn();
    const { unmount } = render(
      <ContextMenu
        position={{ x: 10, y: 10 }}
        onClose={escapeClose}
        items={[{ id: 'copy', label: '复制', onSelect: vi.fn() }]}
      />,
    );
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(escapeClose).toHaveBeenCalledTimes(1);
    unmount();

    const outsideClose = vi.fn();
    render(
      <ContextMenu
        position={{ x: 10, y: 10 }}
        onClose={outsideClose}
        items={[{ id: 'copy', label: '复制', onSelect: vi.fn() }]}
      />,
    );
    fireEvent.pointerDown(document.body);
    expect(outsideClose).toHaveBeenCalledTimes(1);
  });

  it('clamps the menu inside an eight-pixel viewport margin', async () => {
    Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1024 });
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 768 });
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({
      x: 0,
      y: 0,
      top: 0,
      left: 0,
      right: 180,
      bottom: 120,
      width: 180,
      height: 120,
      toJSON: () => ({}),
    });

    render(
      <ContextMenu
        position={{ x: 1000, y: 750 }}
        onClose={vi.fn()}
        items={[{ id: 'copy', label: '复制', onSelect: vi.fn() }]}
      />,
    );

    await waitFor(() => {
      expect(screen.getByRole('menu')).toHaveStyle({ left: '836px', top: '640px' });
    });
  });
});
