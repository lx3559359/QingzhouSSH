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

import { api, asAppError, previewModeFromSearch, tauriApi } from './tauri';

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

  it('routes update scenario URLs to the browser preview API in development', () => {
    expect(previewModeFromSearch('?update=github', true)).toBe('ready');
    expect(previewModeFromSearch('?update=modelscope', true)).toBe('ready');
    expect(previewModeFromSearch('?update=reject', true)).toBe('ready');
    expect(previewModeFromSearch('?update=up_to_date', true)).toBe('ready');
    expect(previewModeFromSearch('?update=unknown', true)).toBeNull();
    expect(previewModeFromSearch('?preview=data-root', true)).toBe('data-root');
    expect(previewModeFromSearch('?update=github', false)).toBeNull();
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
      target: 'content' as const,
      path: '/var/log/app.log',
      keyword: 'error',
      caseSensitive: false,
      contextLines: 2,
      limit: 1000,
      startTime: null,
      endTime: null,
    };

    await api.listTaskDefinitions('server-1');
    await api.getTaskLibrarySnapshot('server-1', true);
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
    await api.listLocalDirectory(null);
    await api.listRemoteDirectory('server-1', '/var/log');
    await api.searchLogs('server-1', logRequest, onEvent);
    await api.readLogResultPage('execution-1', '50', 50);
    await api.downloadLogResult('execution-1', 'result.txt');
    await api.uploadFile(
      'server-1',
      {
        localPath: 'D:\payload.zip',
        remotePath: '/tmp/payload.zip',
        overwrite: false,
        verification: 'balanced',
      },
      onEvent,
    );
    await api.downloadFile(
      'server-1',
      {
        remotePath: '/tmp/result.zip',
        suggestedName: 'result.zip',
        overwrite: false,
        verification: 'balanced',
      },
      onEvent,
    );
    await api.enqueueUploadFile('server-1', {
      localPath: 'D:\\payload.zip',
      remotePath: '/tmp/payload.zip',
      overwrite: false,
      verification: 'balanced',
    });
    await api.enqueueDownloadFile('server-1', {
      remotePath: '/tmp/result.zip',
      suggestedName: 'result.zip',
      overwrite: false,
      verification: 'balanced',
    });
    await api.listTransferJobs('server-1');
    await api.cancelTransferJob('job-1');
    await api.retryTransferJob('job-1');
    await api.listExecutions({ status: 'failed' });
    await api.getExecution('execution-1');

    expect(invoke.mock.calls.map(([command, args]) => [command, args && Object.keys(args)])).toEqual([
      ['list_task_definitions', ['serverId']],
      ['get_task_library_snapshot', ['serverId', 'forceRefresh']],
      ['start_task_execution', ['serverId', 'request', 'onEvent']],
      ['start_custom_execution', ['serverId', 'request', 'onEvent']],
      ['cancel_execution', ['executionId']],
      ['list_local_directory', ['path']],
      ['list_remote_directory', ['serverId', 'path']],
      ['search_logs', ['serverId', 'request', 'onEvent']],
      ['read_log_result_page', ['executionId', 'cursor', 'pageSize']],
      ['download_log_result', ['executionId', 'suggestedName']],
      ['upload_file', ['serverId', 'request', 'onEvent']],
      ['download_file', ['serverId', 'request', 'onEvent']],
      ['enqueue_upload_file', ['serverId', 'request']],
      ['enqueue_download_file', ['serverId', 'request']],
      ['list_transfer_jobs', ['serverId']],
      ['cancel_transfer_job', ['jobId']],
      ['retry_transfer_job', ['jobId']],
      ['list_executions', ['filter']],
      ['get_execution', ['executionId']],
    ]);

    channels[0].onmessage?.({ sequence: 1 });
    channels[0].onmessage?.({ sequence: 1 });
    channels[0].onmessage?.({ sequence: 3 });
    channels[0].onmessage?.({ sequence: 2 });
    expect(received).toEqual([1, 3]);
  });

  it('uses token-bound data migration commands without accepting executable paths', async () => {
    await tauriApi.preflightDataRootMigration('D:\\QingzhouData-New');
    await tauriApi.preflightRetryDataRootMigration();
    await tauriApi.preflightPortableDefaultDataRootMigration();
    await tauriApi.startDataRootMigration('preview-1', 'token-1');
    await tauriApi.getDataRootMigrationStatus();
    await tauriApi.acknowledgeDataRootMigration('migration-1');
    await tauriApi.openDataRootFolder('last_source');

    expect(invoke.mock.calls).toEqual([
      ['preflight_data_root_migration', { targetPath: 'D:\\QingzhouData-New' }],
      ['preflight_retry_data_root_migration'],
      ['preflight_portable_default_data_root_migration'],
      ['start_data_root_migration', { previewId: 'preview-1', confirmationToken: 'token-1' }],
      ['get_data_root_migration_status'],
      ['acknowledge_data_root_migration', { migrationId: 'migration-1' }],
      ['open_data_root_folder', { kind: 'last_source' }],
    ]);
    expect(JSON.stringify(invoke.mock.calls)).not.toContain('executable');
    expect(JSON.stringify(invoke.mock.calls)).not.toContain('sourcePath');
  });

  it('previews and confirms only token-bound component remediation', async () => {
    const received: number[] = [];
    await tauriApi.previewTaskRemediation('server-1', 'network.packet_capture');
    await tauriApi.confirmTaskRemediation(
      'server-1',
      { previewId: 'preview-1', confirmationToken: 'token-1' },
      (event) => received.push(event.sequence),
    );

    expect(invoke.mock.calls.map(([command, args]) => [command, args && Object.keys(args)])).toEqual([
      ['preview_task_remediation', ['serverId', 'taskId']],
      ['confirm_task_remediation', ['serverId', 'request', 'onEvent']],
    ]);
    expect(JSON.stringify(invoke.mock.calls)).not.toContain('sudoPassword');
    expect(JSON.stringify(invoke.mock.calls)).not.toContain('packages');
    expect(JSON.stringify(invoke.mock.calls)).not.toContain('command');

    channels[0].onmessage?.({ sequence: 1 });
    channels[0].onmessage?.({ sequence: 1 });
    channels[0].onmessage?.({ sequence: 2 });
    expect(received).toEqual([1, 2]);
  });

  it('uses typed operations IPC without accepting command text', async () => {
    const preflight = {
      taskId: 'system.overview',
      taskVersion: 2,
      parameters: {},
    };
    const start = {
      ...preflight,
      confirmedPreviewId: null,
    };

    await tauriApi.listOperationsTasks('server-1');
    await tauriApi.preflightOperation('server-1', preflight);
    await tauriApi.startOperation('server-1', start, () => undefined);
    await tauriApi.cancelOperation('run-1');
    await tauriApi.getOperation('run-1');
    await tauriApi.listOperations({ serverId: 'server-1', status: 'failed' });
    await tauriApi.startOperationBatch({
      serverIds: ['server-1', 'server-2'],
      taskId: 'system.overview',
      taskVersion: 2,
      parameters: {},
    });
    await tauriApi.cancelOperationBatch('batch-1');
    await tauriApi.getOperationBatch('batch-1');
    await tauriApi.exportOperationReport('run-1', 'json');
    await tauriApi.exportOperationBatchReport('batch-1', 'txt');
    await tauriApi.previewOperation('server-1', preflight);
    await tauriApi.confirmOperation(
      'server-1',
      { ...preflight, confirmationToken: 'preview-1' },
      () => undefined,
    );
    await tauriApi.listOperationRestorePoints('run-1');
    await tauriApi.rollbackOperation('restore-point-1');
    await tauriApi.inspectUncertainOperation('run-1');
    await tauriApi.cleanupOperationRestoreAssets('restore-point-1');

    expect(invoke.mock.calls.map(([command, args]) => [command, args && Object.keys(args)])).toEqual([
      ['list_operations_tasks', ['serverId']],
      ['preflight_operation', ['serverId', 'request']],
      ['start_operation', ['serverId', 'request', 'onEvent']],
      ['cancel_operation', ['runId']],
      ['get_operation', ['runId']],
      ['list_operations', ['filter']],
      ['start_operation_batch', ['request']],
      ['cancel_operation_batch', ['batchId']],
      ['get_operation_batch', ['batchId']],
      ['export_operation_report', ['runId', 'format']],
      ['export_operation_batch_report', ['batchId', 'format']],
      ['preview_operation', ['serverId', 'request']],
      ['confirm_operation', ['serverId', 'request', 'onEvent']],
      ['list_operation_restore_points', ['runId']],
      ['rollback_operation', ['restorePointId']],
      ['inspect_uncertain_operation', ['runId']],
      ['cleanup_operation_restore_assets', ['restorePointId']],
    ]);
    const serializedCalls = JSON.stringify(invoke.mock.calls);
    for (const forbidden of [
      'commandTemplate',
      '"command"',
      'localPath',
      'remoteScript',
      'sudoPassword',
    ]) {
      expect(serializedCalls).not.toContain(forbidden);
    }
  });

  it('uses bounded personal script IPC without command or path escape hatches', async () => {
    const draft = {
      title: '服务巡检',
      category: '系统维护',
      tags: ['巡检'],
      body: "printf '%s\\n' ok",
      parameters: [],
      timeoutSeconds: 30,
    };
    const metadata = { title: '服务巡检', category: '系统维护', tags: ['巡检'] };
    const events: number[] = [];

    await tauriApi.listPersonalScripts({ query: '巡检', enabled: true });
    await tauriApi.getPersonalScriptForEditor('script-1');
    await tauriApi.listPersonalScriptVersions('script-1');
    await tauriApi.createPersonalScript(draft);
    await tauriApi.savePersonalScriptVersion('script-1', {
      body: draft.body,
      parameters: [],
      timeoutSeconds: 45,
    });
    await tauriApi.updatePersonalScriptMetadata('script-1', metadata);
    await tauriApi.copyPersonalScript('script-1');
    await tauriApi.setPersonalScriptFavorite('script-1', true);
    await tauriApi.setPersonalScriptEnabled('script-1', true);
    await tauriApi.deletePersonalScript('script-1');
    await tauriApi.importPersonalScript('{"schemaVersion":1}');
    await tauriApi.exportPersonalScript('script-1');
    await tauriApi.previewPersonalScriptRun('script-1', 'server-1', { TARGET: 'web' });
    await tauriApi.confirmPersonalScriptRun(
      { previewId: 'preview-1', confirmationToken: 'token-1' },
      (event) => events.push(event.sequence),
    );
    await tauriApi.cancelPersonalScriptRun('preview-1');

    expect(invoke.mock.calls.map(([command, args]) => [command, args && Object.keys(args)])).toEqual([
      ['list_personal_scripts', ['filter']],
      ['get_personal_script_for_editor', ['scriptId']],
      ['list_personal_script_versions', ['scriptId']],
      ['create_personal_script', ['request']],
      ['save_personal_script_version', ['scriptId', 'request']],
      ['update_personal_script_metadata', ['scriptId', 'request']],
      ['copy_personal_script', ['scriptId']],
      ['set_personal_script_favorite', ['scriptId', 'favorite']],
      ['set_personal_script_enabled', ['scriptId', 'enabled']],
      ['delete_personal_script', ['scriptId']],
      ['import_personal_script', ['packageJson']],
      ['export_personal_script', ['scriptId']],
      ['preview_personal_script_run', ['scriptId', 'serverId', 'parameterValues']],
      ['confirm_personal_script_run', ['request', 'onEvent']],
      ['cancel_personal_script_run', ['operationRunId']],
    ]);
    const serialized = JSON.stringify(invoke.mock.calls);
    for (const forbidden of [
      'riskLevel',
      'rollbackAvailable',
      'commandTemplate',
      'localPath',
      'serverIds',
    ]) {
      expect(serialized).not.toContain(forbidden);
    }
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
