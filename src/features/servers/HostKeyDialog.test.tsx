import '@testing-library/jest-dom/vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import type { HostKeyCheck } from '../../api/contracts';
import { HostKeyDialog } from './HostKeyDialog';

const observed = {
  algorithm: 'Ed25519',
  fingerprintSha256: 'SHA256:new-fingerprint',
  rawKeyBase64: 'bmV3',
};

describe('HostKeyDialog', () => {
  it('shows a new host identity and requires explicit trust', async () => {
    const user = userEvent.setup();
    const onApprove = vi.fn();
    const check: HostKeyCheck = {
      decision: 'needs_approval',
      observed,
      trusted: null,
    };
    render(
      <HostKeyDialog
        check={check}
        onApprove={onApprove}
        onContinue={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText('Ed25519')).toBeVisible();
    expect(screen.getByText('SHA256:new-fingerprint')).toBeVisible();
    await user.click(screen.getByRole('button', { name: '信任并继续' }));
    expect(onApprove).toHaveBeenCalledOnce();
  });

  it('allows continuation when the identity is already trusted', async () => {
    const user = userEvent.setup();
    const onContinue = vi.fn();
    render(
      <HostKeyDialog
        check={{ decision: 'trusted', observed, trusted: null }}
        onApprove={vi.fn()}
        onContinue={onContinue}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText('身份已验证')).toBeVisible();
    await user.click(screen.getByRole('button', { name: '继续' }));
    expect(onContinue).toHaveBeenCalledOnce();
  });

  it('blocks trust and continuation when the host key changed', () => {
    const check: HostKeyCheck = {
      decision: 'changed',
      observed,
      trusted: {
        serverId: 'server-1',
        algorithm: 'Ed25519',
        fingerprintSha256: 'SHA256:old-fingerprint',
        rawKeyBase64: 'b2xk',
      },
    };
    render(
      <HostKeyDialog
        check={check}
        onApprove={vi.fn()}
        onContinue={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText('主机身份发生变化')).toBeVisible();
    expect(screen.getByText('SHA256:old-fingerprint')).toBeVisible();
    expect(screen.getByText('SHA256:new-fingerprint')).toBeVisible();
    expect(screen.queryByRole('button', { name: '信任并继续' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '继续' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: '关闭' })).toBeVisible();
  });
});
