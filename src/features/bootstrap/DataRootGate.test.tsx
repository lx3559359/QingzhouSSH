import '@testing-library/jest-dom/vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const open = vi.hoisted(() => vi.fn());
const initializeDataRoot = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/plugin-dialog', () => ({ open }));
vi.mock('../../api/tauri', () => ({ api: { initializeDataRoot } }));

import { DataRootGate } from './DataRootGate';

describe('DataRootGate', () => {
  beforeEach(() => {
    open.mockReset();
    initializeDataRoot.mockReset();
  });

  it('requires an explicit data directory and never offers an AppData default', () => {
    render(
      <DataRootGate status={{ state: 'needs_selection' }} onReady={vi.fn()} />,
    );
    expect(
      screen.getByRole('heading', { name: '选择数据存储位置' }),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: '选择文件夹' })).toBeVisible();
    expect(screen.queryByText(/AppData/i)).not.toBeInTheDocument();
  });

  it('keeps the selected path visible when initialization fails', async () => {
    const user = userEvent.setup();
    open.mockResolvedValue('D:\\QingzhouData');
    initializeDataRoot.mockRejectedValue(new Error('目录不可写'));
    render(
      <DataRootGate status={{ state: 'needs_selection' }} onReady={vi.fn()} />,
    );

    await user.click(screen.getByRole('button', { name: '选择文件夹' }));

    expect(screen.getByText('D:\\QingzhouData')).toBeVisible();
    expect(screen.getByRole('alert')).toHaveTextContent('目录不可写');
  });

  it('reports the initialized root through onReady', async () => {
    const user = userEvent.setup();
    const onReady = vi.fn();
    const ready = { state: 'ready' as const, dataRoot: 'D:\\QingzhouData' };
    open.mockResolvedValue(ready.dataRoot);
    initializeDataRoot.mockResolvedValue(ready);
    render(
      <DataRootGate status={{ state: 'needs_selection' }} onReady={onReady} />,
    );

    await user.click(screen.getByRole('button', { name: '选择文件夹' }));

    expect(onReady).toHaveBeenCalledWith(ready);
  });
});
