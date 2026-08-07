import { describe, expect, it } from 'vitest';

import type { SystemCapabilities, TaskDefinition } from '../../api/contracts';
import { buildInitialParameters, updateDependentParameters } from './parameterDefaults';

const capabilities: SystemCapabilities = {
  osId: 'openeuler', osFamily: 'openeuler', versionId: '24.03', packageManager: 'dnf',
  serviceManager: 'systemd', architecture: 'x86_64', shell: '/bin/bash', commands: ['ip'],
  services: ['nginx.service'], containers: ['web'], dnsServers: ['1.1.1.1'],
  currentTimezone: 'Asia/Shanghai', timezones: ['Asia/Shanghai', 'UTC'],
  currentTime: '2026-08-07T00:20:00+08:00', ntpEnabled: true, ntpSynchronized: true,
  interfaces: [
    { name: 'eth0', isUp: true, isDefault: true, addresses: ['192.0.2.10/24'], gateway4: '192.0.2.1', gateway6: null },
    { name: 'eth1', isUp: true, isDefault: false, addresses: ['198.51.100.10/24'], gateway4: '198.51.100.1', gateway6: null },
  ],
};

const definition: TaskDefinition = {
  id: 'network.ip_change', version: 2, category: 'network', title: '修改 IP 地址', description: '',
  riskLevel: 'dangerous', estimatedSeconds: 180, privilege: 'root_or_passwordless_sudo', scope: 'single_server',
  implementations: [], outputKind: 'key_value',
  parameters: [
    { name: 'interface', label: '网络接口', description: '', kind: { type: 'interfaceName' }, required: true, defaultValue: null, sensitive: false },
    { name: 'cidr', label: '新地址', description: '', kind: { type: 'cidr' }, required: true, defaultValue: null, sensitive: false },
    { name: 'gateway', label: '默认网关', description: '', kind: { type: 'host' }, required: true, defaultValue: null, sensitive: false },
    { name: 'rollbackSeconds', label: '恢复等待', description: '', kind: { type: 'integer', min: 60, max: 300 }, required: true, defaultValue: 120, sensitive: false },
  ],
};

describe('dynamic task parameter defaults', () => {
  it('selects the default-route interface and fills its gateway without guessing a new IP', () => {
    expect(buildInitialParameters(definition, capabilities)).toEqual({
      interface: 'eth0',
      gateway: '192.0.2.1',
      rollbackSeconds: 120,
    });
  });

  it('updates the gateway when the user chooses another detected interface', () => {
    expect(updateDependentParameters(definition.id, { interface: 'eth0', gateway: '192.0.2.1' }, 'interface', 'eth1', capabilities)).toEqual({
      interface: 'eth1',
      gateway: '198.51.100.1',
    });
  });

  it('reflects the detected automatic time-sync state', () => {
    const timeSync: TaskDefinition = {
      ...definition,
      id: 'system.time_sync_change',
      category: 'system',
      title: '设置自动校时',
      parameters: [{
        name: 'enabled', label: '自动校时', description: '', kind: { type: 'boolean' },
        required: true, defaultValue: null, sensitive: false,
      }],
    };
    expect(buildInitialParameters(timeSync, capabilities)).toEqual({ enabled: true });
  });
});
