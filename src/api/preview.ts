import type {
  CreateServerRequest,
  HostKeyCheck,
  HostKeyObservation,
  ServerProfile,
  SystemCapabilities,
} from './contracts';

const previewServer: ServerProfile = {
  id: 'preview-openeuler',
  name: 'openEuler 应用服务器',
  host: '192.168.10.28',
  port: 22,
  username: 'ops',
  authKind: 'private_key',
  credentialId: 'preview-credential',
};

let previewServers = [previewServer];

export const previewApi = {
  bootstrapStatus: async () => ({
    state: 'ready' as const,
    dataRoot: 'D:\\QingzhouSSH\\data',
  }),
  initializeDataRoot: async (path: string) => ({ state: 'ready' as const, dataRoot: path }),
  listServers: async () => previewServers,
  createServer: async (request: CreateServerRequest) => {
    const profile: ServerProfile = {
      id: `preview-${previewServers.length + 1}`,
      name: request.name,
      host: request.host,
      port: request.port,
      username: request.username,
      authKind: request.credential.kind,
      credentialId: `preview-credential-${previewServers.length + 1}`,
    };
    previewServers = [...previewServers, profile];
    return profile;
  },
  inspectHostKey: async (_serverId: string): Promise<HostKeyCheck> => ({
    decision: 'trusted',
    observed: {
      algorithm: 'ssh-ed25519',
      fingerprintSha256: 'SHA256:QingzhouPreviewVerifiedHostKey',
      rawKeyBase64: 'preview-only',
    },
    trusted: {
      serverId: previewServer.id,
      algorithm: 'ssh-ed25519',
      fingerprintSha256: 'SHA256:QingzhouPreviewVerifiedHostKey',
      rawKeyBase64: 'preview-only',
    },
  }),
  trustHostKey: async (_serverId: string, _observation: HostKeyObservation) => undefined,
  testConnection: async (_serverId: string): Promise<SystemCapabilities> => ({
    osId: 'openEuler',
    osFamily: 'RHEL / 国产 Linux',
    versionId: '24.03 LTS',
    packageManager: 'dnf',
    serviceManager: 'systemd',
    architecture: 'x86_64',
    shell: '/bin/bash',
  }),
};

export const dataRootPreviewApi = {
  ...previewApi,
  bootstrapStatus: async () => ({ state: 'needs_selection' as const }),
};
