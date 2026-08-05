import type {
  ConfirmPersonalScriptRunRequest,
  CreatePersonalScriptRequest,
  ExecutionEvent,
  PersonalScriptDetails,
  PersonalScriptListFilter,
  PersonalScriptRunPreview,
  PersonalScriptRunResult,
  PersonalScriptSummary,
  PersonalScriptVersion,
  SavePersonalScriptVersionRequest,
  ScriptPackageExport,
  UpdatePersonalScriptMetadataRequest,
} from '../../../api/contracts';

export interface PersonalScriptApi {
  listPersonalScripts(filter: PersonalScriptListFilter): Promise<PersonalScriptSummary[]>;
  getPersonalScriptForEditor(scriptId: string): Promise<PersonalScriptDetails | null>;
  listPersonalScriptVersions(scriptId: string): Promise<PersonalScriptVersion[]>;
  createPersonalScript(request: CreatePersonalScriptRequest): Promise<PersonalScriptDetails>;
  savePersonalScriptVersion(
    scriptId: string,
    request: SavePersonalScriptVersionRequest,
  ): Promise<PersonalScriptVersion>;
  updatePersonalScriptMetadata(
    scriptId: string,
    request: UpdatePersonalScriptMetadataRequest,
  ): Promise<void>;
  copyPersonalScript(scriptId: string): Promise<PersonalScriptDetails>;
  setPersonalScriptFavorite(scriptId: string, favorite: boolean): Promise<void>;
  setPersonalScriptEnabled(scriptId: string, enabled: boolean): Promise<void>;
  deletePersonalScript(scriptId: string): Promise<void>;
  importPersonalScript(packageJson: string): Promise<PersonalScriptDetails>;
  exportPersonalScript(scriptId: string): Promise<ScriptPackageExport>;
  previewPersonalScriptRun(
    scriptId: string,
    serverId: string,
    parameterValues: Record<string, unknown>,
  ): Promise<PersonalScriptRunPreview>;
  confirmPersonalScriptRun(
    request: ConfirmPersonalScriptRunRequest,
    onEvent: (event: ExecutionEvent) => void,
  ): Promise<PersonalScriptRunResult>;
  cancelPersonalScriptRun(operationRunId: string): Promise<void>;
}

export interface ScriptEditorDraft extends CreatePersonalScriptRequest {}
