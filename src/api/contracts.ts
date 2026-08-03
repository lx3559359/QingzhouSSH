export type BootstrapStatus =
  | { state: 'needs_selection' }
  | { state: 'ready'; dataRoot: string };

export type CredentialInput =
  | { kind: 'password'; password: string }
  | { kind: 'private_key'; privateKey: string; passphrase: string | null };

export interface CreateServerRequest {
  name: string;
  host: string;
  port: number;
  username: string;
  credential: CredentialInput;
}

export interface ServerProfile {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  authKind: 'password' | 'private_key';
  credentialId: string;
}

export interface HostKeyObservation {
  algorithm: string;
  fingerprintSha256: string;
  rawKeyBase64: string;
}

export interface HostKeyCheck {
  decision: 'trusted' | 'needs_approval' | 'changed';
  observed: HostKeyObservation;
  trusted: (HostKeyObservation & { serverId: string }) | null;
}

export interface SystemCapabilities {
  osId: string;
  osFamily: string;
  versionId: string | null;
  packageManager: string | null;
  serviceManager: string;
  architecture: string;
  shell: string;
}
