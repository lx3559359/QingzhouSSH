import '@testing-library/jest-dom/vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  listServers: vi.fn(),
  createServer: vi.fn(),
  inspectHostKey: vi.fn(),
  trustHostKey: vi.fn(),
  testConnection: vi.fn(),
}));

vi.mock('../../api/tauri', () => ({ api: apiMocks }));

import type {
  HostKeyCheck,
  ServerProfile,
  SystemCapabilities,
} from '../../api/contracts';
import { ServerListPage } from './ServerListPage';

const server: ServerProfile = {
  id: 'server-1',
  name: '生产环境',
  host: '10.0.0.8',
  port: 22,
  username: 'ops',
  authKind: 'password',
  credentialId: 'credential-1',
};

const needsApproval: HostKeyCheck = {
  decision: 'needs_approval',
  observed: {
    algorithm: 'ssh-ed25519',
    fingerprintSha256: 'SHA256:new-fingerprint',
    rawKeyBase64: 'new-key',
  },
  trusted: null,
};

const capabilities: SystemCapabilities = {
  osId: 'openEuler',
  osFamily: '国产 Linux',
  versionId: '24.03',
  packageManager: 'dnf',
  serviceManager: 'systemd',
  architecture: 'x86_64',
  shell: '/bin/bash',
};

async function submitServer(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole('button', { name: '添加服务器' }));
  await user.type(screen.getByLabelText('名称'), '生产环境');
  await user.type(screen.getByLabelText('服务器地址'), '10.0.0.8');
  await user.type(screen.getByLabelText('用户名'), 'ops');
  await user.type(
    screen.getByLabelText('密码', { selector: 'input[type="password"]' }),
    'secret',
  );
  await user.click(screen.getByRole('button', { name: '保存并检查身份' }));
}

describe('ServerListPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    apiMocks.listServers.mockResolvedValue([]);
    apiMocks.createServer.mockResolvedValue(server);
    apiMocks.inspectHostKey.mockResolvedValue(needsApproval);
    apiMocks.trustHostKey.mockResolvedValue(undefined);
    apiMocks.testConnection.mockResolvedValue(capabilities);
  });

  it('creates, inspects, trusts, and tests a server in that order', async () => {
    const user = userEvent.setup();
    render(<ServerListPage />);
    await screen.findByText('还没有服务器');

    await submitServer(user);

    expect(await screen.findByRole('heading', { name: '确认服务器身份' })).toBeVisible();
    expect(apiMocks.createServer).toHaveBeenCalledTimes(1);
    expect(apiMocks.inspectHostKey).toHaveBeenCalledWith('server-1');
    expect(apiMocks.createServer.mock.invocationCallOrder[0]).toBeLessThan(
      apiMocks.inspectHostKey.mock.invocationCallOrder[0],
    );

    await user.click(screen.getByRole('button', { name: '信任并继续' }));

    expect(await screen.findByText('openEuler 24.03')).toBeVisible();
    expect(screen.getByText('国产 Linux')).toBeVisible();
    expect(screen.getByText('dnf')).toBeVisible();
    expect(screen.getByText('systemd')).toBeVisible();
    expect(screen.getByText('x86_64')).toBeVisible();
    expect(apiMocks.trustHostKey).toHaveBeenCalledWith('server-1', needsApproval.observed);
    expect(apiMocks.testConnection).toHaveBeenCalledWith('server-1');
    expect(apiMocks.trustHostKey.mock.invocationCallOrder[0]).toBeLessThan(
      apiMocks.testConnection.mock.invocationCallOrder[0],
    );
  });

  it('blocks connection when the trusted host key has changed', async () => {
    const user = userEvent.setup();
    apiMocks.inspectHostKey.mockResolvedValue({
      ...needsApproval,
      decision: 'changed',
      trusted: {
        serverId: 'server-1',
        algorithm: 'ssh-ed25519',
        fingerprintSha256: 'SHA256:old-fingerprint',
        rawKeyBase64: 'old-key',
      },
    } satisfies HostKeyCheck);

    render(<ServerListPage />);
    await screen.findByText('还没有服务器');
    await submitServer(user);

    expect(await screen.findByRole('heading', { name: '主机身份发生变化' })).toBeVisible();
    expect(screen.getByText('SHA256:old-fingerprint')).toBeVisible();
    expect(screen.getByText('SHA256:new-fingerprint')).toBeVisible();
    expect(screen.queryByRole('button', { name: '信任并继续' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '继续' })).not.toBeInTheDocument();

    await waitFor(() => expect(apiMocks.testConnection).not.toHaveBeenCalled());
    expect(apiMocks.trustHostKey).not.toHaveBeenCalled();
  });
});
