import '@testing-library/jest-dom/vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { AddServerDialog } from './AddServerDialog';

describe('AddServerDialog', () => {
  it('defaults the port to 22 and rejects values outside 1-65535', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(<AddServerDialog onSubmit={onSubmit} onCancel={vi.fn()} />);
    const port = screen.getByRole('spinbutton', { name: '端口' });
    expect(port).toHaveValue(22);

    await user.clear(port);
    await user.type(port, '0');
    await user.click(screen.getByRole('button', { name: '保存并检查身份' }));
    expect(screen.getByRole('alert')).toHaveTextContent('端口必须在 1 到 65535 之间');
    expect(onSubmit).not.toHaveBeenCalled();

    await user.clear(port);
    await user.type(port, '65536');
    await user.click(screen.getByRole('button', { name: '保存并检查身份' }));
    expect(screen.getByRole('alert')).toHaveTextContent('端口必须在 1 到 65535 之间');
  });

  it('keeps password and private-key inputs mutually exclusive', async () => {
    const user = userEvent.setup();
    render(<AddServerDialog onSubmit={vi.fn()} onCancel={vi.fn()} />);
    expect(screen.getByLabelText('密码', { selector: 'input[type="password"]' })).toBeVisible();
    expect(screen.queryByLabelText('私钥内容')).not.toBeInTheDocument();

    await user.click(screen.getByRole('radio', { name: '私钥' }));

    expect(
      screen.queryByLabelText('密码', { selector: 'input[type="password"]' }),
    ).not.toBeInTheDocument();
    expect(screen.getByLabelText('私钥内容')).toBeVisible();
    expect(screen.getByLabelText('私钥口令（可选）')).toBeVisible();
  });

  it('submits a typed request and clears the password after success', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(<AddServerDialog onSubmit={onSubmit} onCancel={vi.fn()} />);

    await user.type(screen.getByLabelText('名称'), '网站服务器');
    await user.type(screen.getByLabelText('服务器地址'), '10.0.0.8');
    await user.type(screen.getByLabelText('用户名'), 'root');
    await user.type(
      screen.getByLabelText('密码', { selector: 'input[type="password"]' }),
      'canary-password',
    );
    await user.click(screen.getByRole('button', { name: '保存并检查身份' }));

    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledWith({
        name: '网站服务器',
        host: '10.0.0.8',
        port: 22,
        username: 'root',
        credential: { kind: 'password', password: 'canary-password' },
      });
    });
    expect(
      screen.getByLabelText('密码', { selector: 'input[type="password"]' }),
    ).toHaveValue('');
  });
});
