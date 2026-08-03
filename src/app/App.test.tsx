import '@testing-library/jest-dom/vitest';
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { App } from './App';

describe('App', () => {
  it('presents QingzhouSSH as a task tool without a terminal entry', () => {
    render(<App />);
    expect(screen.getByRole('heading', { name: '轻舟 SSH' })).toBeVisible();
    expect(screen.getByText('安全地完成 Linux 操作')).toBeVisible();
    expect(screen.queryByText('打开终端')).not.toBeInTheDocument();
  });
});
