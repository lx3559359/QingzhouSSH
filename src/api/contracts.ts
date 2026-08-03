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

export interface NodePosition {
  x: number;
  y: number;
}

export type NumericOperator =
  | 'equal'
  | 'not_equal'
  | 'less_than'
  | 'less_than_or_equal'
  | 'greater_than'
  | 'greater_than_or_equal';

export type EqualityOperator = 'equal' | 'not_equal';

export type WorkflowCondition =
  | { kind: 'exitCode'; operator: NumericOperator; value: number }
  | { kind: 'resultField'; path: string; operator: EqualityOperator; value: string | number | boolean }
  | { kind: 'outputContains'; text: string; negated: boolean };

export type WorkflowNodeConfig =
  | { type: 'start' }
  | {
      type: 'task';
      taskId: string;
      taskVersion: number;
      parameters: Record<string, unknown>;
    }
  | { type: 'custom'; mode: 'command' | 'script'; content: string; timeoutSeconds: number }
  | {
      type: 'upload';
      localPath: string;
      remotePath: string;
      overwrite: boolean;
      createRestorePoint: boolean;
    }
  | { type: 'download'; remotePath: string; suggestedName: string; overwrite: boolean }
  | {
      type: 'logSearch';
      path: string;
      keyword: string;
      caseSensitive: boolean;
      contextLines: number;
      limit: number;
      startTime: string | null;
      endTime: string | null;
    }
  | { type: 'condition'; sourceNodeId: string; predicate: WorkflowCondition }
  | { type: 'stop'; message: string };

export interface WorkflowNode {
  id: string;
  name: string;
  position: NodePosition;
  config: WorkflowNodeConfig;
}

export type WorkflowEdgeBranch = 'success' | 'true' | 'false';

export interface WorkflowEdge {
  from: string;
  to: string;
  branch: WorkflowEdgeBranch;
}

export interface WorkflowDraft {
  id: string | null;
  name: string;
  description: string;
  nodes: WorkflowNode[];
  edges: WorkflowEdge[];
}

export interface WorkflowDefinition extends Omit<WorkflowDraft, 'id'> {
  id: string;
  version: number;
  checksumSha256: string;
}

export interface WorkflowSummary {
  id: string;
  name: string;
  description: string;
  currentVersion: number;
  createdAt: number;
  updatedAt: number;
}

export type WorkflowRunStatus =
  | 'queued'
  | 'running'
  | 'paused'
  | 'succeeded'
  | 'cancelled'
  | 'uncertain'
  | 'rolled_back'
  | 'rollback_failed';

export type WorkflowNodeStatus =
  | 'pending'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'uncertain'
  | 'skipped';

export type WorkflowRestorePointStatus =
  | 'creating'
  | 'available'
  | 'failed'
  | 'rolling_back'
  | 'rolled_back'
  | 'expired';

export interface WorkflowRunRecord {
  id: string;
  workflowId: string;
  workflowVersion: number;
  serverId: string;
  status: WorkflowRunStatus;
  currentNodeId: string | null;
  createdAt: number;
  startedAt: number | null;
  finishedAt: number | null;
  durationMs: number | null;
  errorCategory: string | null;
  errorMessage: string | null;
  retryable: boolean;
}

export interface WorkflowNodeRun {
  runId: string;
  nodeId: string;
  attempt: number;
  status: WorkflowNodeStatus;
  executionId: string | null;
  startedAt: number | null;
  finishedAt: number | null;
  durationMs: number | null;
  exitCode: number | null;
  result: unknown | null;
  outputSummary: string | null;
  errorMessage: string | null;
  retryable: boolean;
}

export interface WorkflowRestorePoint {
  id: string;
  runId: string;
  nodeId: string;
  remotePath: string;
  relativePath: string | null;
  originalExisted: boolean;
  sizeBytes: number | null;
  sha256: string | null;
  status: WorkflowRestorePointStatus;
  applicability: Record<string, unknown>;
  errorMessage: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface WorkflowRunEventRecord {
  runId: string;
  sequence: number;
  eventType: string;
  payload: Record<string, unknown>;
  emittedAt: number;
}

export interface WorkflowRunDetails {
  run: WorkflowRunRecord;
  nodeRuns: WorkflowNodeRun[];
  restorePoints: WorkflowRestorePoint[];
  events: WorkflowRunEventRecord[];
}

export interface StartWorkflowRunRequest {
  workflowId: string;
  workflowVersion: number | null;
  serverId: string;
  dangerousConfirmed: boolean;
}

export interface WorkflowRunFilter {
  workflowId?: string;
  serverId?: string;
  status?: WorkflowRunStatus;
  createdFrom?: number;
  createdTo?: number;
}

export type WorkflowDiagnosticCode =
  | 'graph_limit'
  | 'duplicate_node'
  | 'start_count'
  | 'start_edges'
  | 'stop_edges'
  | 'missing_node'
  | 'self_edge'
  | 'duplicate_edge'
  | 'invalid_branch'
  | 'condition_branches'
  | 'cycle'
  | 'unreachable_node'
  | 'no_terminal_path'
  | 'invalid_parameters';

export interface WorkflowDiagnostic {
  code: WorkflowDiagnosticCode;
  nodeId: string | null;
  message: string;
}

export interface WorkflowValidationReport {
  valid: boolean;
  startNodeId: string | null;
  diagnostics: WorkflowDiagnostic[];
}

type WorkflowEventBase = { sequence: number; emittedAt: number };

export type WorkflowEvent = WorkflowEventBase &
  (
    | { type: 'runStarted'; runId: string; workflowId: string; serverId: string }
    | { type: 'runStatusChanged'; runId: string; status: WorkflowRunStatus; message: string | null }
    | { type: 'nodeStarted'; runId: string; nodeId: string; attempt: number }
    | {
        type: 'nodeStatusChanged';
        runId: string;
        nodeId: string;
        attempt: number;
        status: WorkflowNodeStatus;
        executionId: string | null;
        message: string | null;
      }
    | { type: 'conditionEvaluated'; runId: string; nodeId: string; result: boolean }
    | {
        type: 'restorePointChanged';
        runId: string;
        nodeId: string;
        restorePointId: string;
        status: string;
      }
    | { type: 'finished'; runId: string; status: WorkflowRunStatus; durationMs: number }
  );

export interface AppErrorDto {
  code: string;
  message: string;
}

export type UpdateSource = 'github' | 'modelscope';

export type UpdatePhase =
  | 'idle'
  | 'checking'
  | 'up_to_date'
  | 'available'
  | 'downloading'
  | 'downloaded'
  | 'installing'
  | 'failed';

export type StoredUpdateCheckStatus = 'up_to_date' | 'available' | 'failed';

export interface StoredUpdateCheckResult {
  status: StoredUpdateCheckStatus;
  version: string | null;
  source: UpdateSource | null;
  message: string | null;
}

export interface AvailableUpdate {
  version: string;
  notes: string;
  publishedAt: string | null;
  size: number;
  buildId: string;
  source: UpdateSource;
  sourceLabel: string;
}

export interface StagedUpdate {
  version: string;
  relativePath: string;
  sha256: string;
  size: number;
}

export interface UpdateStatus {
  currentVersion: string;
  phase: UpdatePhase;
  autoCheck: boolean;
  lastCheckedAt: number | null;
  lastResult: StoredUpdateCheckResult | null;
  release: AvailableUpdate | null;
  fallbackReason: string | null;
  staged: StagedUpdate | null;
  lastError: string | null;
}

export interface UpdateProgressEvent {
  sequence: number;
  downloadedBytes: number;
  totalBytes: number | null;
}
