import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({ invoke }));

import { api } from './tauri';

describe('Tauri API wrapper', () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue(undefined);
  });

  it('uses the fixed command names and camelCase argument keys', async () => {
    const request = {
      name: '网站服务器',
      host: '127.0.0.1',
      port: 22,
      username: 'tester',
      credential: { kind: 'password' as const, password: 'secret' },
    };
    const observation = {
      algorithm: 'Ed25519',
      fingerprintSha256: 'SHA256:abc',
      rawKeyBase64: 'YWJj',
    };

    await api.bootstrapStatus();
    await api.initializeDataRoot('D:\\QingzhouData');
    await api.listServers();
    await api.createServer(request);
    await api.inspectHostKey('server-1');
    await api.trustHostKey('server-1', observation);
    await api.testConnection('server-1');

    expect(invoke.mock.calls).toEqual([
      ['bootstrap_status'],
      ['initialize_data_root', { path: 'D:\\QingzhouData' }],
      ['list_servers'],
      ['create_server', { request }],
      ['inspect_server_host_key', { serverId: 'server-1' }],
      ['trust_server_host_key', { serverId: 'server-1', observation }],
      ['test_server_connection', { serverId: 'server-1' }],
    ]);
  });
});
