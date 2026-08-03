import { invoke } from '@tauri-apps/api/core';

import type {
  BootstrapStatus,
  CreateServerRequest,
  HostKeyCheck,
  HostKeyObservation,
  ServerProfile,
  SystemCapabilities,
} from './contracts';
import { dataRootPreviewApi, previewApi } from './preview';

const tauriApi = {
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
