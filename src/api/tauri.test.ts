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

import { api, asAppError, tauriApi } from './tauri';

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

  it('uses all workflow command names, camelCase arguments and monotonic channels', async () => {
    const draft = {
      id: null,
      name: 'deploy',
      description: 'reference',
      nodes: [
        {
          id: '11111111-1111-4111-8111-111111111111',
          name: 'start',
          position: { x: 20, y: 40 },
          config: { type: 'start' as const },
        },
      ],
      edges: [],
    };
    const received: number[] = [];
    const onEvent = (event: { sequence: number }) => received.push(event.sequence);

    await tauriApi.listWorkflows();
    await tauriApi.getWorkflow('workflow-1', 2);
    await tauriApi.saveWorkflow(draft);
    await tauriApi.deleteWorkflow('workflow-1');
    await tauriApi.validateWorkflow(draft);
    await tauriApi.startWorkflowRun(
      {
        workflowId: 'workflow-1',
        workflowVersion: 2,
        serverId: 'server-1',
        dangerousConfirmed: true,
      },
      onEvent,
    );
    await tauriApi.cancelWorkflowRun('run-1');
    await tauriApi.retryWorkflowNode('run-1', true, onEvent);
    await tauriApi.listWorkflowRuns({ serverId: 'server-1', status: 'paused' });
    await tauriApi.getWorkflowRun('run-1');
    await tauriApi.rollbackWorkflowRun('run-1', true);
    await tauriApi.cleanupWorkflowRestorePoints('run-1');
    await tauriApi.exportWorkflowDiagnostics('run-1');

    expect(invoke.mock.calls.map(([command, args]) => [command, args && Object.keys(args)])).toEqual([
      ['list_workflows', undefined],
      ['get_workflow', ['workflowId', 'version']],
      ['save_workflow', ['draft']],
      ['delete_workflow', ['workflowId']],
      ['validate_workflow', ['draft']],
      ['start_workflow_run', ['request', 'onEvent']],
      ['cancel_workflow_run', ['runId']],
      ['retry_workflow_node', ['runId', 'dangerousConfirmed', 'onEvent']],
      ['list_workflow_runs', ['filter']],
      ['get_workflow_run', ['runId']],
      ['rollback_workflow_run', ['runId', 'dangerousConfirmed']],
      ['cleanup_workflow_restore_points', ['runId']],
      ['export_workflow_diagnostics', ['runId']],
    ]);

    channels[0].onmessage?.({ sequence: 1 });
    channels[0].onmessage?.({ sequence: 1 });
    channels[0].onmessage?.({ sequence: 4 });
    channels[0].onmessage?.({ sequence: 3 });
    expect(received).toEqual([1, 4]);
  });

  it('uses all updater command names and filters non-monotonic progress', async () => {
    const received: number[] = [];

    await tauriApi.getUpdateStatus();
    await tauriApi.setAutoUpdateCheck(false);
    await tauriApi.checkForUpdate(true);
    await tauriApi.downloadUpdate((event) => received.push(event.sequence));
    await tauriApi.installUpdate(true);
    await tauriApi.clearDownloadedUpdate();

    expect(invoke.mock.calls.map(([command, args]) => [command, args && Object.keys(args)])).toEqual([
      ['get_update_status', undefined],
      ['set_auto_update_check', ['enabled']],
      ['check_for_update', ['manual']],
      ['download_update', ['onEvent']],
      ['install_update', ['confirmed']],
      ['clear_downloaded_update', undefined],
    ]);

    channels[0].onmessage?.({ sequence: 1, downloadedBytes: 4, totalBytes: 20 });
    channels[0].onmessage?.({ sequence: 1, downloadedBytes: 2, totalBytes: 20 });
    channels[0].onmessage?.({ sequence: 3, downloadedBytes: 20, totalBytes: 20 });
    channels[0].onmessage?.({ sequence: 2, downloadedBytes: 10, totalBytes: 20 });
    expect(received).toEqual([1, 3]);
  });

  it('normalizes backend error DTOs without exposing arbitrary values', () => {
    expect(asAppError({ code: 'validation', message: 'invalid workflow' })).toEqual({
      code: 'validation',
      message: 'invalid workflow',
    });
    expect(asAppError(new Error('transport failed'))).toEqual({
      code: 'unknown',
      message: 'transport failed',
    });
    expect(asAppError({ secret: 'do-not-render' })).toEqual({
      code: 'unknown',
      message: '操作失败',
    });
  });
});
