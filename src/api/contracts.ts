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
  commands: string[];
}

export type ExecutionStatus =
  | 'queued'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'uncertain';

export type TaskCategory = 'system' | 'service' | 'logs' | 'advanced';
export type RiskLevel = 'safe' | 'caution' | 'dangerous';
export type OutputKind = 'text' | 'table' | 'key_value' | 'log_matches';

export type ParameterKind =
  | { type: 'string'; minLength: number; maxLength: number; multiline: boolean }
  | { type: 'integer'; min: number; max: number }
  | { type: 'boolean' }
  | { type: 'enum'; options: string[] }
  | { type: 'absolutePath' }
  | { type: 'serviceName' }
  | { type: 'timeRange' };

export interface ParameterDefinition {
  name: string;
  label: string;
  description: string;
  kind: ParameterKind;
  required: boolean;
  defaultValue: unknown | null;
  sensitive: boolean;
}

export interface CompatibilityPredicate {
  osFamilies: string[];
  serviceManagers: string[];
  requiredCommands: string[];
}

export interface TaskImplementation {
  id: string;
  compatibility: CompatibilityPredicate;
}

export interface TaskDefinition {
  id: string;
  version: number;
  category: TaskCategory;
  title: string;
  description: string;
  riskLevel: RiskLevel;
  parameters: ParameterDefinition[];
  implementations: TaskImplementation[];
  outputKind: OutputKind;
}

export interface TaskAvailability {
  definition: TaskDefinition;
  compatible: boolean;
  reason: string | null;
}

export interface TaskExecutionRequest {
  taskId: string;
  parameters: Record<string, unknown>;
  dangerousConfirmed: boolean;
}

export interface CustomExecutionRequest {
  mode: 'command' | 'script';
  content: string;
  timeoutSeconds: number;
  dangerousConfirmed: boolean;
}

export interface ExecutionParameter {
  name: string;
  displayValue: string;
  sensitive: boolean;
}

export interface ExecutionFile {
  id: string;
  relativePath: string;
  purpose: string;
  sizeBytes: number;
  sha256: string;
}

export interface ExecutionRecord {
  id: string;
  serverId: string;
  taskId: string;
  taskVersion: number;
  category: string;
  status: ExecutionStatus;
  createdAt: number;
  startedAt: number | null;
  finishedAt: number | null;
  durationMs: number | null;
  exitCode: number | null;
  errorCategory: string | null;
  errorMessage: string | null;
  retryable: boolean;
  parametersSummary: string | null;
  outputSummary: string | null;
  remoteProcessGroup: string | null;
}

export interface ExecutionDetails {
  record: ExecutionRecord;
  parameters: ExecutionParameter[];
  files: ExecutionFile[];
}

type ExecutionEventBase = { sequence: number; emittedAt: number };

export type ExecutionEvent = ExecutionEventBase &
  (
    | { type: 'started'; executionId: string; startedAt: number }
    | { type: 'stdout'; text: string; totalBytes: number }
    | { type: 'stderr'; text: string; totalBytes: number }
    | {
        type: 'progress';
        transferred: number;
        total: number | null;
        percent: number | null;
      }
    | { type: 'fileProduced'; file: ExecutionFile }
    | {
        type: 'finished';
        status: ExecutionStatus;
        exitCode: number | null;
        durationMs: number;
        result: unknown | null;
      }
    | { type: 'failed'; category: string; message: string; retryable: boolean }
  );

export interface LogSearchRequest {
  path: string;
  keyword: string;
  caseSensitive: boolean;
  contextLines: number;
  limit: number;
  startTime: string | null;
  endTime: string | null;
}

export interface LogMatch {
  path: string;
  lineNumber: number;
  kind: 'match' | 'context';
  timestamp: string | null;
  text: string;
}

export interface LogResultPage {
  items: LogMatch[];
  nextCursor: string | null;
}

export interface UploadRequest {
  localPath: string;
  remotePath: string;
  overwrite: boolean;
}

export interface DownloadRequest {
  remotePath: string;
  suggestedName: string;
  overwrite: boolean;
}

export interface ExecutionFilter {
  serverId?: string;
  category?: string;
  status?: ExecutionStatus;
  createdFrom?: number;
  createdTo?: number;
}
