import { Channel, invoke } from '@tauri-apps/api/core';

import type {
  BootstrapStatus,
  CreateServerRequest,
  ConfirmTaskRemediationRequest,
  CustomExecutionRequest,
  DownloadRequest,
  DirectoryListing,
  ExecutionDetails,
  ExecutionEvent,
  ExecutionFilter,
  ExecutionRecord,
  HostKeyCheck,
  HostKeyObservation,
  LogResultPage,
  LogSearchRequest,
  OperationEvent,
  OperationConfirmRequest,
  OperationFilter,
  OperationBatchDetails,
  OperationBatchRequest,
  ReportFormat,
  OperationPreflightRequest,
  OperationPreview,
  OperationRunDetails,
  OperationRunRecord,
  OperationRecoveryResult,
  OperationRestoreDetails,
  OperationStartRequest,
  ServerProfile,
  SystemCapabilities,
  TaskAvailability,
  TaskRemediationPreview,
  TaskExecutionRequest,
  UploadRequest,
  ExecutionFile,
  StartWorkflowRunRequest,
  WorkflowDefinition,
  WorkflowDraft,
  WorkflowEvent,
  WorkflowRunDetails,
  WorkflowRunFilter,
  WorkflowRunRecord,
  WorkflowSummary,
  WorkflowValidationReport,
  UpdateProgressEvent,
  UpdateStatus,
  ConfirmPersonalScriptRunRequest,
  CreatePersonalScriptRequest,
  PersonalScriptDetails,
  PersonalScriptListFilter,
  PersonalScriptRunPreview,
  PersonalScriptRunResult,
  PersonalScriptSummary,
  PersonalScriptVersion,
  SavePersonalScriptVersionRequest,
  ScriptPackageExport,
  UpdatePersonalScriptMetadataRequest,
} from './contracts';
import { dataRootPreviewApi, previewApi } from './preview';
export { normalizeAppError as asAppError } from './errors';

export type ExecutionEventHandler = (event: ExecutionEvent) => void;
export type OperationEventHandler = (event: OperationEvent) => void;

function createMonotonicChannel<T extends { sequence: number }>(onEvent: (event: T) => void) {
  const channel = new Channel<T>();
  let lastSequence = 0;
  channel.onmessage = (event) => {
    if (event.sequence <= lastSequence) return;
    lastSequence = event.sequence;
    onEvent(event);
  };
  return channel;
}

function createEventChannel(onEvent: ExecutionEventHandler) {
  return createMonotonicChannel(onEvent);
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
  previewTaskRemediation: (serverId: string, taskId: string) =>
    invoke<TaskRemediationPreview>('preview_task_remediation', { serverId, taskId }),
  confirmTaskRemediation: (
    serverId: string,
    request: ConfirmTaskRemediationRequest,
    onEvent: ExecutionEventHandler,
  ) => invoke<TaskAvailability>('confirm_task_remediation', {
    serverId,
    request,
    onEvent: createEventChannel(onEvent),
  }),
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
  listLocalDirectory: (path: string | null) =>
    invoke<DirectoryListing>('list_local_directory', { path }),
  listRemoteDirectory: (serverId: string, path: string) =>
    invoke<DirectoryListing>('list_remote_directory', { serverId, path }),
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
  listOperationsTasks: (serverId: string) =>
    invoke<TaskAvailability[]>('list_operations_tasks', { serverId }),
  preflightOperation: (serverId: string, request: OperationPreflightRequest) =>
    invoke<OperationPreview>('preflight_operation', { serverId, request }),
  startOperation: (
    serverId: string,
    request: OperationStartRequest,
    onEvent: OperationEventHandler,
  ) =>
    invoke<OperationRunDetails>('start_operation', {
      serverId,
      request,
      onEvent: createMonotonicChannel(onEvent),
    }),
  cancelOperation: (runId: string) =>
    invoke<void>('cancel_operation', { runId }),
  getOperation: (runId: string) =>
    invoke<OperationRunDetails | null>('get_operation', { runId }),
  listOperations: (filter: OperationFilter) =>
    invoke<OperationRunRecord[]>('list_operations', { filter }),
  startOperationBatch: (request: OperationBatchRequest) =>
    invoke<OperationBatchDetails>('start_operation_batch', { request }),
  cancelOperationBatch: (batchId: string) =>
    invoke<void>('cancel_operation_batch', { batchId }),
  getOperationBatch: (batchId: string) =>
    invoke<OperationBatchDetails | null>('get_operation_batch', { batchId }),
  exportOperationReport: (runId: string, format: ReportFormat) =>
    invoke<ExecutionFile>('export_operation_report', { runId, format }),
  exportOperationBatchReport: (batchId: string, format: ReportFormat) =>
    invoke<ExecutionFile>('export_operation_batch_report', { batchId, format }),
  previewOperation: (serverId: string, request: OperationPreflightRequest) =>
    invoke<OperationPreview>('preview_operation', { serverId, request }),
  confirmOperation: (
    serverId: string,
    request: OperationConfirmRequest,
    onEvent: OperationEventHandler,
  ) =>
    invoke<OperationRunDetails>('confirm_operation', {
      serverId,
      request,
      onEvent: createMonotonicChannel(onEvent),
    }),
  listOperationRestorePoints: (runId: string) =>
    invoke<OperationRestoreDetails[]>('list_operation_restore_points', { runId }),
  rollbackOperation: (restorePointId: string) =>
    invoke<OperationRecoveryResult>('rollback_operation', { restorePointId }),
  inspectUncertainOperation: (runId: string) =>
    invoke<OperationRecoveryResult>('inspect_uncertain_operation', { runId }),
  cleanupOperationRestoreAssets: (restorePointId: string) =>
    invoke<OperationRestoreDetails>('cleanup_operation_restore_assets', { restorePointId }),
  listPersonalScripts: (filter: PersonalScriptListFilter) =>
    invoke<PersonalScriptSummary[]>('list_personal_scripts', { filter }),
  getPersonalScriptForEditor: (scriptId: string) =>
    invoke<PersonalScriptDetails | null>('get_personal_script_for_editor', { scriptId }),
  listPersonalScriptVersions: (scriptId: string) =>
    invoke<PersonalScriptVersion[]>('list_personal_script_versions', { scriptId }),
  createPersonalScript: (request: CreatePersonalScriptRequest) =>
    invoke<PersonalScriptDetails>('create_personal_script', { request }),
  savePersonalScriptVersion: (scriptId: string, request: SavePersonalScriptVersionRequest) =>
    invoke<PersonalScriptVersion>('save_personal_script_version', { scriptId, request }),
  updatePersonalScriptMetadata: (
    scriptId: string,
    request: UpdatePersonalScriptMetadataRequest,
  ) => invoke<void>('update_personal_script_metadata', { scriptId, request }),
  copyPersonalScript: (scriptId: string) =>
    invoke<PersonalScriptDetails>('copy_personal_script', { scriptId }),
  setPersonalScriptFavorite: (scriptId: string, favorite: boolean) =>
    invoke<void>('set_personal_script_favorite', { scriptId, favorite }),
  setPersonalScriptEnabled: (scriptId: string, enabled: boolean) =>
    invoke<void>('set_personal_script_enabled', { scriptId, enabled }),
  deletePersonalScript: (scriptId: string) =>
    invoke<void>('delete_personal_script', { scriptId }),
  importPersonalScript: (packageJson: string) =>
    invoke<PersonalScriptDetails>('import_personal_script', { packageJson }),
  exportPersonalScript: (scriptId: string) =>
    invoke<ScriptPackageExport>('export_personal_script', { scriptId }),
  previewPersonalScriptRun: (
    scriptId: string,
    serverId: string,
    parameterValues: Record<string, unknown>,
  ) => invoke<PersonalScriptRunPreview>('preview_personal_script_run', {
    scriptId,
    serverId,
    parameterValues,
  }),
  confirmPersonalScriptRun: (
    request: ConfirmPersonalScriptRunRequest,
    onEvent: ExecutionEventHandler,
  ) => invoke<PersonalScriptRunResult>('confirm_personal_script_run', {
    request,
    onEvent: createEventChannel(onEvent),
  }),
  cancelPersonalScriptRun: (operationRunId: string) =>
    invoke<void>('cancel_personal_script_run', { operationRunId }),
  listWorkflows: () => invoke<WorkflowSummary[]>('list_workflows'),
  getWorkflow: (workflowId: string, version: number | null) =>
    invoke<WorkflowDefinition | null>('get_workflow', { workflowId, version }),
  saveWorkflow: (draft: WorkflowDraft) =>
    invoke<WorkflowDefinition>('save_workflow', { draft }),
  deleteWorkflow: (workflowId: string) =>
    invoke<boolean>('delete_workflow', { workflowId }),
  validateWorkflow: (draft: WorkflowDraft) =>
    invoke<WorkflowValidationReport>('validate_workflow', { draft }),
  startWorkflowRun: (
    request: StartWorkflowRunRequest,
    onEvent: (event: WorkflowEvent) => void,
  ) =>
    invoke<WorkflowRunDetails>('start_workflow_run', {
      request,
      onEvent: createMonotonicChannel(onEvent),
    }),
  cancelWorkflowRun: (runId: string) =>
    invoke<void>('cancel_workflow_run', { runId }),
  retryWorkflowNode: (
    runId: string,
    dangerousConfirmed: boolean,
    onEvent: (event: WorkflowEvent) => void,
  ) =>
    invoke<WorkflowRunDetails>('retry_workflow_node', {
      runId,
      dangerousConfirmed,
      onEvent: createMonotonicChannel(onEvent),
    }),
  listWorkflowRuns: (filter: WorkflowRunFilter) =>
    invoke<WorkflowRunRecord[]>('list_workflow_runs', { filter }),
  getWorkflowRun: (runId: string) =>
    invoke<WorkflowRunDetails | null>('get_workflow_run', { runId }),
  rollbackWorkflowRun: (runId: string, dangerousConfirmed: boolean) =>
    invoke<WorkflowRunDetails>('rollback_workflow_run', { runId, dangerousConfirmed }),
  cleanupWorkflowRestorePoints: (runId: string) =>
    invoke<number>('cleanup_workflow_restore_points', { runId }),
  exportWorkflowDiagnostics: (runId: string) =>
    invoke<ExecutionFile>('export_workflow_diagnostics', { runId }),
  getUpdateStatus: () => invoke<UpdateStatus>('get_update_status'),
  setAutoUpdateCheck: (enabled: boolean) =>
    invoke<UpdateStatus>('set_auto_update_check', { enabled }),
  checkForUpdate: (manual: boolean) =>
    invoke<UpdateStatus>('check_for_update', { manual }),
  downloadUpdate: (onEvent: (event: UpdateProgressEvent) => void) =>
    invoke<UpdateStatus>('download_update', {
      onEvent: createMonotonicChannel(onEvent),
    }),
  installUpdate: (confirmed: boolean) =>
    invoke<UpdateStatus>('install_update', { confirmed }),
  clearDownloadedUpdate: () => invoke<UpdateStatus>('clear_downloaded_update'),
};

export function previewModeFromSearch(
  search: string,
  development: boolean,
): 'ready' | 'data-root' | null {
  if (!development) return null;
  const parameters = new URLSearchParams(search);
  const preview = parameters.get('preview');
  if (preview === 'ready' || preview === 'data-root') return preview;
  const update = parameters.get('update');
  return update === 'github' ||
    update === 'modelscope' ||
    update === 'reject' ||
    update === 'up_to_date'
    ? 'ready'
    : null;
}

const previewRequested =
  typeof window !== 'undefined'
    ? previewModeFromSearch(window.location.search, import.meta.env.DEV)
    : null;

export const api =
  previewRequested === 'ready'
    ? previewApi
    : previewRequested === 'data-root'
      ? dataRootPreviewApi
      : tauriApi;
