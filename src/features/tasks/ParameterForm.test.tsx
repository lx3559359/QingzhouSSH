import '@testing-library/jest-dom/vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import type { ParameterDefinition, SystemCapabilities } from '../../api/contracts';
import { ParameterForm } from './ParameterForm';

const capabilities: SystemCapabilities = {
  osId: 'openeuler',
  osFamily: 'openeuler',
  versionId: '24.03',
  packageManager: 'dnf',
  serviceManager: 'systemd',
  architecture: 'x86_64',
  shell: '/bin/bash',
  commands: ['ip', 'systemctl', 'timedatectl', 'docker'],
  services: ['nginx.service', 'sshd.service'],
  containers: ['web', 'database'],
  interfaces: [
    {
      name: 'eth0',
      isUp: true,
      isDefault: true,
      addresses: ['192.0.2.10/24'],
      gateway4: '192.0.2.1',
      gateway6: null,
    },
    {
      name: 'eth1',
      isUp: false,
      isDefault: false,
      addresses: [],
      gateway4: null,
      gateway6: null,
    },
  ],
  dnsServers: ['1.1.1.1'],
  currentTimezone: 'Asia/Shanghai',
  currentTime: '2026-08-07T00:20:00+08:00',
  ntpEnabled: true,
  ntpSynchronized: true,
  timezones: ['Asia/Shanghai', 'UTC'],
};

const definitions: ParameterDefinition[] = [
  {
    name: 'interface',
    label: '网络接口',
    description: '从服务器接口中选择目标网卡',
    kind: { type: 'interfaceName' },
    required: true,
    defaultValue: null,
    sensitive: false,
  },
  {
    name: 'service',
    label: '服务名称',
    description: '从服务器服务中选择',
    kind: { type: 'serviceName' },
    required: true,
    defaultValue: null,
    sensitive: false,
  },
  {
    name: 'container',
    label: '容器名称',
    description: '从服务器容器中选择',
    kind: { type: 'containerName' },
    required: true,
    defaultValue: null,
    sensitive: false,
  },
  {
    name: 'timezone',
    label: '新时区',
    description: 'IANA 时区',
    kind: { type: 'timezone' },
    required: true,
    defaultValue: null,
    sensitive: false,
  },
];

describe('ParameterForm dynamic discovery', () => {
  it('turns discovered interfaces, services, containers and timezones into choices', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <ParameterForm
        definitions={definitions}
        values={{ interface: 'eth0', timezone: 'Asia/Shanghai' }}
        capabilities={capabilities}
        onChange={onChange}
      />,
    );

    expect(screen.getByRole('option', { name: /eth0.*默认.*192\.0\.2\.10\/24/ })).toBeVisible();
    expect(screen.getByRole('option', { name: 'nginx.service' })).toBeVisible();
    expect(screen.getByRole('option', { name: 'web' })).toBeVisible();
    expect(screen.getByRole('option', { name: /Asia\/Shanghai.*当前/ })).toBeVisible();

    await user.selectOptions(screen.getByLabelText('服务名称'), 'nginx.service');
    expect(onChange).toHaveBeenCalledWith('service', 'nginx.service');
  });
});
