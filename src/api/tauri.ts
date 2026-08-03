import { Channel, invoke } from '@tauri-apps/api/core';

import type {
  BootstrapStatus,
  CreateServerRequest,
  CustomExecutionRequest,
  DownloadRequest,
  ExecutionDetails,
  ExecutionEvent,
  ExecutionFilter,
  ExecutionRecord,
  HostKeyCheck,
  HostKeyObservation,
  LogResultPage,
  LogSearchRequest,
  ServerProfile,
  SystemCapabilities,
  TaskAvailability,
  TaskExecutionRequest,
  UploadRequest,
} from './contracts';
import { dataRootPreviewApi, previewApi } from './preview';

export type ExecutionEventHandler = (event: ExecutionEvent) => void;

function createEventChannel(onEvent: ExecutionEventHandler) {
  const channel = new Channel<ExecutionEvent>();
  let lastSequence = 0;
  channel.onmessage = (event) => {
    if (event.sequence <= lastSequence) return;
    lastSequence = event.sequence;
    onEvent(event);
  };
  return channel;
}

export const tauriApi = {
  bootstrapStatus: () => invoke<BootstrapStatus>('bootstrap_status'),
  initializeDataRoot: (path: string) =>
    invoke<BootstrapStatus>('initialize_data_root', { path }),
  listServers: () => invoke<ServerProfile[]>('list_servers'),
  createServer: (request: CreateServerRequest) =>
    invoke<ServerProfile>('create_server', { request }),
  inspectHostKey: (serverId: string) =>
    invoke<HostKeyCheck>('inspect_server_host_key', { serverId }),
  trustHostKey: (serverId: string, observation: HostKeyObservation) =>
    invoke<void>('trust_server_host_key', { serverId, observation }),
  testConnection: (serverId: string) =>
    invoke<SystemCapabilities>('test_server_connection', { serverId }),
  listTaskDefinitions: (serverId: string) =>
    invoke<TaskAvailability[]>('list_task_definitions', { serverId }),
  startTaskExecution: (
    serverId: string,
    request: TaskExecutionRequest,
    onEvent: ExecutionEventHandler,
  ) =>
    invoke<ExecutionDetails>('start_task_execution', {
      serverId,
      request,
      onEvent: createEventChannel(onEvent),
    }),
  startCustomExecution: (
    serverId: string,
    request: CustomExecutionRequest,
    onEvent: ExecutionEventHandler,
  ) =>
    invoke<ExecutionDetails>('start_custom_execution', {
      serverId,
      request,
      onEvent: createEventChannel(onEvent),
    }),
  cancelExecution: (executionId: string) =>
    invoke<void>('cancel_execution', { executionId }),
  searchLogs: (
    serverId: string,
    request: LogSearchRequest,
    onEvent: ExecutionEventHandler,
  ) =>
    invoke<ExecutionDetails>('search_logs', {
      serverId,
      request,
      onEvent: createEventChannel(onEvent),
    }),
  readLogResultPage: (executionId: string, cursor: string | null, pageSize: number) =>
    invoke<LogResultPage>('read_log_result_page', { executionId, cursor, pageSize }),
  downloadLogResult: (executionId: string, suggestedName: string) =>
    invoke<string>('download_log_result', { executionId, suggestedName }),
  uploadFile: (
    serverId: string,
    request: UploadRequest,
    onEvent: ExecutionEventHandler,
  ) =>
    invoke<ExecutionDetails>('upload_file', {
      serverId,
      request,
      onEvent: createEventChannel(onEvent),
    }),
  downloadFile: (
    serverId: string,
    request: DownloadRequest,
    onEvent: ExecutionEventHandler,
  ) =>
    invoke<ExecutionDetails>('download_file', {
      serverId,
      request,
      onEvent: createEventChannel(onEvent),
    }),
  listExecutions: (filter: ExecutionFilter) =>
    invoke<ExecutionRecord[]>('list_executions', { filter }),
  getExecution: (executionId: string) =>
    invoke<ExecutionDetails | null>('get_execution', { executionId }),
};

const previewRequested =
  import.meta.env.DEV && typeof window !== 'undefined'
    ? new URLSearchParams(window.location.search).get('preview')
    : null;

export const api =
  previewRequested === 'ready'
    ? previewApi
    : previewRequested === 'data-root'
      ? dataRootPreviewApi
      : tauriApi;
