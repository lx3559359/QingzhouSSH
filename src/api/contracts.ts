export type BootstrapStatus =
  | { state: 'needs_selection' }
  | {
      state: 'ready';
      dataRoot: string;
      dataRootSource: DataRootSource;
      dataRootMutable: boolean;
      lastDataMigration: DataMigrationJournal | null;
    };

export type DataRootSource =
  | 'environment'
  | 'portable_custom'
  | 'portable_default'
  | 'registry'
  | 'needs_selection';

export type DataMigrationPhase =
  | 'prepared'
  | 'copying'
  | 'verifying'
  | 'switched'
  | 'completed'
  | 'failed';

export interface DataMigrationJournal {
  schemaVersion: number;
  migrationId: string;
  source: string;
  target: string;
  sourceMode: DataRootSource;
  parentPid: number;
  fileCount: number;
  totalBytes: number;
  copiedFiles: number;
  copiedBytes: number;
  phase: DataMigrationPhase;
  errorSummary: string | null;
  startedAt: number;
  updatedAt: number;
  acknowledged: boolean;
}

export interface DataMigrationPreview {
  previewId: string;
  confirmationToken: string;
  expiresAt: number;
  source: string;
  target: string;
  fileCount: number;
  totalBytes: number;
  requiredBytes: number;
  availableBytes: number;
  oldRootWillBeKept: true;
  retryable: boolean;
}

export type ReadyBootstrapStatus = Extract<BootstrapStatus, { state: 'ready' }>;

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
  services: string[];
  containers: string[];
  interfaces: NetworkInterfaceCapability[];
  dnsServers: string[];
  currentTimezone: string | null;
  currentTime: string | null;
  ntpEnabled: boolean | null;
  ntpSynchronized: boolean | null;
  timezones: string[];
}

export interface NetworkInterfaceCapability {
  name: string;
  isUp: boolean;
  isDefault: boolean;
  addresses: string[];
  gateway4: string | null;
  gateway6: string | null;
}

export type ExecutionStatus =
  | 'queued'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'uncertain';

export type TaskCategory =
  | 'system'
  | 'storage'
  | 'network'
  | 'security'
  | 'service'
  | 'logs'
  | 'web'
  | 'container'
  | 'script'
  | 'advanced';
export type RiskLevel = 'safe' | 'caution' | 'dangerous';
export type OutputKind = 'text' | 'table' | 'key_value' | 'log_matches';
export type PrivilegeRequirement = 'current_user' | 'root_or_passwordless_sudo';
export type ExecutionScope = 'single_server' | 'read_only_batch';

export type ParameterKind =
  | { type: 'string'; minLength: number; maxLength: number; multiline: boolean }
  | { type: 'integer'; min: number; max: number }
  | { type: 'boolean' }
  | { type: 'enum'; options: string[] }
  | { type: 'absolutePath' }
  | { type: 'serviceName' }
  | { type: 'timeRange' }
  | { type: 'host' }
  | { type: 'port' }
  | { type: 'interfaceName' }
  | { type: 'timezone' }
  | { type: 'cidr' }
  | { type: 'containerName' }
  | { type: 'fileMode' }
  | { type: 'cronExpression' }
  | { type: 'managedId' }
  | { type: 'multiSelect'; options: string[]; maxItems: number }
  | { type: 'serviceMultiSelect'; maxItems: number };

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
  preflightSteps: TaskStepMetadata[];
  previewSteps: TaskStepMetadata[];
  backupPlan: BackupPlan | null;
  executionSteps: TaskStepMetadata[];
  verifySteps: TaskStepMetadata[];
  rollbackPlan: RollbackPlan | null;
  resultParser: ResultParserKind;
}

export interface TaskStepMetadata {
  id: string;
  title: string;
  timeoutSeconds: number;
  outputLimitBytes: number;
}

export type BackupItemKind =
  | 'remote_file'
  | 'command_snapshot'
  | 'managed_block'
  | 'runtime_state';

export interface BackupItemDefinition {
  id: string;
  kind: BackupItemKind;
}

export interface BackupPlan {
  items: BackupItemDefinition[];
}

export interface RollbackPlan {
  steps: TaskStepMetadata[];
  automaticOnFailure: boolean;
}

export type ResultParserKind =
  | 'text'
  | 'key_value'
  | 'table'
  | 'health_summary'
  | 'network_probe'
  | 'service_status'
  | 'container_status';

export interface TaskDefinition {
  id: string;
  version: number;
  category: TaskCategory;
  title: string;
  description: string;
  riskLevel: RiskLevel;
  estimatedSeconds: number;
  privilege: PrivilegeRequirement;
  scope: ExecutionScope;
  parameters: ParameterDefinition[];
  implementations: TaskImplementation[];
  outputKind: OutputKind;
}

export interface TaskAvailability {
  definition: TaskDefinition;
  state: TaskAvailabilityState;
  summary: string;
  missingCommands: string[];
  remediation: TaskRemediationSummary | null;
  library: ToolLibraryMetadata;
}

export interface TaskLibrarySnapshot {
  tasks: TaskAvailability[];
  capabilities: SystemCapabilities;
  detectedAt: number;
  cacheExpiresAt: number;
}

export type TaskAvailabilityState =
  | 'ready'
  | 'remediable'
  | 'permission_blocked'
  | 'unsupported';

export interface TaskRemediationSummary {
  packageManager: string;
  missingCommands: string[];
  packages: string[];
}

export interface TaskRemediationPreview {
  previewId: string;
  confirmationToken: string;
  expiresAt: number;
  taskId: string;
  implementationId: string;
  missingCommands: string[];
  packages: string[];
  packageManager: string;
  permissionState: TaskAvailabilityState;
  commandSummary: string;
}

export interface ConfirmTaskRemediationRequest {
  previewId: string;
  confirmationToken: string;
}

export type ToolSource = 'builtin_task' | 'reviewed_command';

export type ToolLibraryCategory =
  | 'recommended_recent'
  | 'daily_inspection'
  | 'performance'
  | 'storage'
  | 'network'
  | 'web_service'
  | 'security_login'
  | 'service_management'
  | 'container'
  | 'system_settings';

export interface ToolLibraryMetadata {
  source: ToolSource;
  primaryCategory: ToolLibraryCategory;
  keywords: string[];
  noviceAliases: string[];
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

export interface ScriptScanWarning {
  code: string;
  message: string;
  lineNumber: number;
}

export interface ScriptScanSummary {
  lineCount: number;
  characterCount: number;
  bodySha256: string;
  warningCount: number;
  warnings: ScriptScanWarning[];
}

export interface PersonalScriptDefinition {
  id: string;
  title: string;
  category: string;
  tags: string[];
  isFavorite: boolean;
  isEnabled: boolean;
  activeVersionId: string;
  createdAt: number;
  updatedAt: number;
  deletedAt: number | null;
}

export interface PersonalScriptVersion {
  id: string;
  definitionId: string;
  versionNumber: number;
  body: string;
  bodySha256: string;
  parameters: ParameterDefinition[];
  scanSummary: ScriptScanSummary;
  timeoutSeconds: number;
  createdAt: number;
}

export interface PersonalScriptDetails {
  definition: PersonalScriptDefinition;
  activeVersion: PersonalScriptVersion;
}

export interface PersonalScriptSummary {
  id: string;
  title: string;
  category: string;
  tags: string[];
  isFavorite: boolean;
  isEnabled: boolean;
  activeVersionId: string;
  activeVersionNumber: number;
  bodySha256: string;
  updatedAt: number;
}

export interface PersonalScriptListFilter {
  query?: string;
  category?: string;
  tag?: string;
  favorite?: boolean;
  enabled?: boolean;
}

export interface CreatePersonalScriptRequest {
  title: string;
  category: string;
  tags: string[];
  body: string;
  parameters: ParameterDefinition[];
  timeoutSeconds: number;
}

export interface SavePersonalScriptVersionRequest {
  body: string;
  parameters: ParameterDefinition[];
  timeoutSeconds: number;
}

export interface UpdatePersonalScriptMetadataRequest {
  title: string;
  category: string;
  tags: string[];
}

export interface ScriptPackageExport {
  relativePath: string;
  sha256: string;
  sizeBytes: number;
}

export interface PersonalScriptRunPreview {
  previewId: string;
  confirmationToken: string;
  expiresAt: number;
  serverId: string;
  scriptDefinitionId: string;
  scriptVersionId: string;
  scriptVersionNumber: number;
  title: string;
  riskLevel: 'dangerous';
  automaticRollbackAvailable: false;
  warning: string;
  lineCount: number;
  characterCount: number;
  bodySha256: string;
  parameterNames: string[];
  scanWarnings: ScriptScanWarning[];
  timeoutSeconds: number;
}

export interface ConfirmPersonalScriptRunRequest {
  previewId: string;
  confirmationToken: string;
}

export interface PersonalScriptRunResult {
  operationRunId: string;
  scriptDefinitionId: string;
  scriptVersionId: string;
  execution: ExecutionDetails;
}

export type OperationStatus =
  | 'validating'
  | 'preflighting'
  | 'preview_ready'
  | 'waiting_confirmation'
  | 'backing_up'
  | 'running'
  | 'verifying'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'uncertain'
  | 'rollback_available'
  | 'rolling_back'
  | 'rolled_back'
  | 'rollback_partial'
  | 'rollback_failed';

export type OperationPhase = 'preflight' | 'backup' | 'execute' | 'verify' | 'rollback';
export type OperationStepStatus =
  | 'pending'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'uncertain'
  | 'skipped';

export interface OperationPreflightRequest {
  taskId: string;
  taskVersion: number;
  parameters: Record<string, unknown>;
}

export interface OperationStartRequest extends OperationPreflightRequest {
  confirmedPreviewId: string | null;
}

export interface OperationConfirmRequest extends OperationPreflightRequest {
  confirmationToken: string;
}

export interface OperationPreviewServer {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
}

export interface OperationDisconnectRisk {
  mayDisconnect: boolean;
  explanation: string | null;
  automaticRecoverySeconds: number | null;
}

export interface OperationPreview {
  previewId: string;
  serverId: string;
  taskId: string;
  taskVersion: number;
  implementationId: string;
  riskLevel: RiskLevel;
  privilege: PrivilegeRequirement;
  scope: ExecutionScope;
  status: OperationStatus;
  stepTitles: string[];
  estimatedSeconds: number;
  confirmationToken: string | null;
  server: OperationPreviewServer;
  permissionSummary: string;
  currentStateSummary: string;
  targetStateSummary: string;
  backupSummary: string[];
  disconnectRisk: OperationDisconnectRisk;
}

export interface OperationRunRecord {
  id: string;
  serverId: string;
  taskId: string;
  taskVersion: number;
  riskLevel: RiskLevel;
  status: OperationStatus;
  parametersSummary: string | null;
  result: unknown | null;
  errorCategory: string | null;
  errorMessage: string | null;
  createdAt: number;
  updatedAt: number;
  finishedAt: number | null;
}

export interface OperationStepRecord {
  runId: string;
  phase: OperationPhase;
  stepIndex: number;
  stepId: string;
  title: string;
  status: OperationStepStatus;
  executionId: string | null;
  outputSummary: string | null;
  errorMessage: string | null;
  startedAt: number | null;
  finishedAt: number | null;
}

export interface OperationRunDetails {
  run: OperationRunRecord;
  steps: OperationStepRecord[];
}

export type OperationRestorePointStatus =
  | 'creating'
  | 'available'
  | 'rolling_back'
  | 'rolled_back'
  | 'partial'
  | 'failed'
  | 'expired'
  | 'cleanup_pending';

export type OperationRestoreItemStatus =
  | 'pending'
  | 'available'
  | 'rolling_back'
  | 'rolled_back'
  | 'failed'
  | 'skipped';

export interface OperationRestorePoint {
  id: string;
  operationRunId: string;
  serverId: string;
  taskId: string;
  status: OperationRestorePointStatus;
  localRelativeDir: string;
  remoteAssetId: string | null;
  expiresAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface OperationRestoreItem {
  id: string;
  restorePointId: string;
  ordinal: number;
  itemKind: BackupItemKind;
  localRelativePath: string | null;
  sha256: string | null;
  originalMetadata: unknown;
  status: OperationRestoreItemStatus;
  errorSummary: string | null;
}

export interface OperationRestoreDetails {
  point: OperationRestorePoint;
  items: OperationRestoreItem[];
}

export interface OperationRecoveryResult {
  operation: OperationRunDetails;
  whatHappened: string;
  serverMayHaveChanged: boolean;
  stateConfirmed: boolean;
  nextStep: string;
  restorePoint: OperationRestorePoint | null;
  technicalDetails: string | null;
}

export interface OperationFilter {
  serverId?: string;
  taskId?: string;
  status?: OperationStatus;
  createdFrom?: number;
  createdTo?: number;
}

export type OperationBatchStatus =
  | 'queued'
  | 'running'
  | 'succeeded'
  | 'partial'
  | 'failed'
  | 'cancelled';

export type OperationBatchItemStatus =
  | 'queued'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'cancelled';

export interface OperationBatchRequest {
  serverIds: string[];
  taskId: string;
  taskVersion: number;
  parameters: Record<string, unknown>;
}

export interface OperationBatchRecord {
  id: string;
  taskId: string;
  taskVersion: number;
  status: OperationBatchStatus;
  createdAt: number;
  finishedAt: number | null;
}

export interface OperationBatchItemRecord {
  batchId: string;
  serverId: string;
  operationRunId: string | null;
  status: OperationBatchItemStatus;
  errorMessage: string | null;
}

export interface OperationBatchDetails {
  batch: OperationBatchRecord;
  items: OperationBatchItemRecord[];
}

export type ReportFormat = 'json' | 'txt';

export type OperationEvent = ExecutionEvent;

type ExecutionEventBase = { sequence: number; emittedAt: number };

export type TransferPhase = 'connecting' | 'transferring' | 'verifying' | 'finalizing';

export type ExecutionEvent = ExecutionEventBase &
  (
    | { type: 'started'; executionId: string; startedAt: number }
    | { type: 'stdout'; text: string; totalBytes: number }
    | { type: 'stderr'; text: string; totalBytes: number }
    | {
        type: 'progress';
        phase: TransferPhase;
        transferred: number;
        total: number | null;
        percent: number | null;
        bytesPerSecond: number | null;
        averageBytesPerSecond: number | null;
        etaSeconds: number | null;
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

export type LogSearchTarget = 'content' | 'filename';

export interface LogSearchRequest {
  target: LogSearchTarget;
  path: string;
  keyword: string;
  caseSensitive: boolean;
  contextLines: number;
  limit: number;
  startTime: string | null;
  endTime: string | null;
}

export type BrowserEntryKind = 'directory' | 'file' | 'symlink' | 'other';

export interface BrowserEntry {
  name: string;
  path: string;
  kind: BrowserEntryKind;
  size: number | null;
  modifiedAt: number | null;
}

export interface DirectoryListing {
  path: string;
  parent: string | null;
  entries: BrowserEntry[];
}

export interface LogMatch {
  path: string;
  lineNumber: number;
  kind: 'match' | 'context';
  timestamp: string | null;
  text: string;
}

export type ContentSearchResult = LogMatch & { resultType: 'content' };

export interface RemoteFileMatch {
  resultType: 'file';
  path: string;
  name: string;
  size: number | null;
  modifiedAt: number | null;
}

export type SearchResultItem = ContentSearchResult | RemoteFileMatch;

export interface LogResultPage {
  items: SearchResultItem[];
  nextCursor: string | null;
}

export type VerificationPolicy = 'balanced' | 'strict' | 'transport_only';

export interface UploadRequest {
  localPath: string;
  remotePath: string;
  overwrite: boolean;
  verification: VerificationPolicy;
}

export interface DownloadRequest {
  remotePath: string;
  suggestedName: string;
  overwrite: boolean;
  verification: VerificationPolicy;
}

export type TransferDirection = 'upload' | 'download';
export type TransferJobStatus =
  | 'queued'
  | 'connecting'
  | 'transferring'
  | 'verifying'
  | 'finalizing'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'uncertain';

export interface TransferJob {
  id: string;
  executionId: string | null;
  serverId: string;
  direction: TransferDirection;
  sourcePath: string;
  targetPath: string;
  overwrite: boolean;
  verification: VerificationPolicy;
  status: TransferJobStatus;
  transferred: number;
  total: number | null;
  percent: number | null;
  bytesPerSecond: number | null;
  averageBytesPerSecond: number | null;
  etaSeconds: number | null;
  attemptCount: number;
  maxAttempts: number;
  cancelRequested: boolean;
  retryable: boolean;
  errorCategory: string | null;
  errorMessage: string | null;
  sha256: string | null;
  location: string | null;
  createdAt: number;
  updatedAt: number;
  startedAt: number | null;
  finishedAt: number | null;
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
  retryable?: boolean;
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
