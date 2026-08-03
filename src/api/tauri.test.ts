import { beforeEach, describe, expect, it, vi } from 'vitest';

const { invoke, channels, MockChannel } = vi.hoisted(() => {
  const channels = [] as Array<{ onmessage?: (message: unknown) => void }>;
  class MockChannel {
    onmessage?: (message: unknown) => void;

    constructor() {
      channels.push(this);
    }
  }
  return { invoke: vi.fn(), channels, MockChannel };
});

vi.mock('@tauri-apps/api/core', () => ({ Channel: MockChannel, invoke }));

import { api } from './tauri';

describe('Tauri API wrapper', () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue(undefined);
    channels.length = 0;
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

  it('uses milestone two command names and forwards only monotonic channel events', async () => {
    const received: number[] = [];
    const onEvent = (event: { sequence: number }) => received.push(event.sequence);
    const taskRequest = {
      taskId: 'system.overview',
      parameters: {},
      dangerousConfirmed: false,
    };
    const logRequest = {
      path: '/var/log/app.log',
      keyword: 'error',
      caseSensitive: false,
      contextLines: 2,
      limit: 1000,
      startTime: null,
      endTime: null,
    };

    await api.listTaskDefinitions('server-1');
    await api.startTaskExecution('server-1', taskRequest, onEvent);
    await api.startCustomExecution(
      'server-1',
      {
        mode: 'command',
        content: 'uptime',
        timeoutSeconds: 30,
        dangerousConfirmed: true,
      },
      onEvent,
    );
    await api.cancelExecution('execution-1');
    await api.searchLogs('server-1', logRequest, onEvent);
    await api.readLogResultPage('execution-1', '50', 50);
    await api.downloadLogResult('execution-1', 'result.txt');
    await api.uploadFile(
      'server-1',
      { localPath: 'D:\payload.zip', remotePath: '/tmp/payload.zip', overwrite: false },
      onEvent,
    );
    await api.downloadFile(
      'server-1',
      { remotePath: '/tmp/result.zip', suggestedName: 'result.zip', overwrite: false },
      onEvent,
    );
    await api.listExecutions({ status: 'failed' });
    await api.getExecution('execution-1');

    expect(invoke.mock.calls.map(([command, args]) => [command, args && Object.keys(args)])).toEqual([
      ['list_task_definitions', ['serverId']],
      ['start_task_execution', ['serverId', 'request', 'onEvent']],
      ['start_custom_execution', ['serverId', 'request', 'onEvent']],
      ['cancel_execution', ['executionId']],
      ['search_logs', ['serverId', 'request', 'onEvent']],
      ['read_log_result_page', ['executionId', 'cursor', 'pageSize']],
      ['download_log_result', ['executionId', 'suggestedName']],
      ['upload_file', ['serverId', 'request', 'onEvent']],
      ['download_file', ['serverId', 'request', 'onEvent']],
      ['list_executions', ['filter']],
      ['get_execution', ['executionId']],
    ]);

    channels[0].onmessage?.({ sequence: 1 });
    channels[0].onmessage?.({ sequence: 1 });
    channels[0].onmessage?.({ sequence: 3 });
    channels[0].onmessage?.({ sequence: 2 });
    expect(received).toEqual([1, 3]);
  });
});
