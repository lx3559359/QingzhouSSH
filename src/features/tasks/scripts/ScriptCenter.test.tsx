import '@testing-library/jest-dom/vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import type { PersonalScriptDetails, PersonalScriptSummary, ServerProfile } from '../../../api/contracts';
import { ScriptCenter } from './ScriptCenter';
import type { PersonalScriptApi } from './types';

const server: ServerProfile = {
  id: 'server-1',
  name: '测试服务器',
  host: '127.0.0.1',
  port: 22,
  username: 'tester',
  authKind: 'password',
  credentialId: 'credential-1',
};

function scriptDetails(enabled = false, body = 'echo ok'): PersonalScriptDetails {
  return {
    definition: {
      id: 'script-1',
      title: '服务巡检',
      category: '系统维护',
      tags: ['巡检'],
      isFavorite: false,
      isEnabled: enabled,
      activeVersionId: 'version-1',
      createdAt: 1,
      updatedAt: 1,
      deletedAt: null,
    },
    activeVersion: {
      id: 'version-1',
      definitionId: 'script-1',
      versionNumber: 1,
      body,
      bodySha256: 'a'.repeat(64),
      parameters: [],
      scanSummary: {
        lineCount: 1,
        characterCount: body.length,
        bodySha256: 'a'.repeat(64),
        warningCount: 0,
        warnings: [],
      },
      timeoutSeconds: 30,
      createdAt: 1,
    },
  };
}

function summary(details: PersonalScriptDetails): PersonalScriptSummary {
  return {
    id: details.definition.id,
    title: details.definition.title,
    category: details.definition.category,
    tags: details.definition.tags,
    isFavorite: details.definition.isFavorite,
    isEnabled: details.definition.isEnabled,
    activeVersionId: details.activeVersion.id,
    activeVersionNumber: details.activeVersion.versionNumber,
    bodySha256: details.activeVersion.bodySha256,
    updatedAt: details.definition.updatedAt,
  };
}

function fixtureApi(initial: PersonalScriptDetails | null = null): PersonalScriptApi {
  let current = initial;
  return {
    listPersonalScripts: vi.fn(async () => current ? [summary(current)] : []),
    getPersonalScriptForEditor: vi.fn(async () => current),
    listPersonalScriptVersions: vi.fn(async () => current ? [current.activeVersion] : []),
    createPersonalScript: vi.fn(async (request) => {
      current = scriptDetails(false, request.body);
      current.definition.title = request.title;
      return current;
    }),
    savePersonalScriptVersion: vi.fn(async (_id, request) => {
      if (!current) throw new Error('missing');
      current.activeVersion = {
        ...current.activeVersion,
        id: 'version-2',
        versionNumber: 2,
        body: request.body,
      };
      current.definition.activeVersionId = 'version-2';
      return current.activeVersion;
    }),
    updatePersonalScriptMetadata: vi.fn(async (_id, request) => {
      if (current) current.definition = { ...current.definition, ...request };
    }),
    copyPersonalScript: vi.fn(async () => scriptDetails(false)),
    setPersonalScriptFavorite: vi.fn(async (_id, favorite) => {
      if (current) current.definition.isFavorite = favorite;
    }),
    setPersonalScriptEnabled: vi.fn(async (_id, enabled) => {
      if (current) current.definition.isEnabled = enabled;
    }),
    deletePersonalScript: vi.fn(async () => { current = null; }),
    importPersonalScript: vi.fn(async () => {
      current = scriptDetails(false);
      return current;
    }),
    exportPersonalScript: vi.fn(async () => ({
      relativePath: 'downloads/scripts/script-test.json',
      sha256: 'a'.repeat(64),
      sizeBytes: 100,
    })),
    previewPersonalScriptRun: vi.fn(async () => ({
      previewId: 'preview-1',
      confirmationToken: 'token-1',
      expiresAt: Date.now() + 10_000,
      serverId: server.id,
      scriptDefinitionId: 'script-1',
      scriptVersionId: 'version-1',
      scriptVersionNumber: 1,
      title: '服务巡检',
      riskLevel: 'dangerous' as const,
      automaticRollbackAvailable: false as const,
      warning: '不可自动回滚：请确认目标服务器和参数后再运行。',
      lineCount: 1,
      characterCount: 7,
      bodySha256: 'a'.repeat(64),
      parameterNames: [],
      scanWarnings: [],
      timeoutSeconds: 30,
    })),
    confirmPersonalScriptRun: vi.fn(async () => ({
      operationRunId: 'preview-1',
      scriptDefinitionId: 'script-1',
      scriptVersionId: 'version-1',
      execution: {
        record: {
          id: 'execution-1', serverId: server.id, taskId: 'script.personal', taskVersion: 1,
          category: 'advanced', status: 'succeeded' as const, createdAt: 1, startedAt: 1, finishedAt: 2,
          durationMs: 1, exitCode: 0, errorCategory: null, errorMessage: null, retryable: false,
          parametersSummary: null, outputSummary: 'ok', remoteProcessGroup: null,
        },
        parameters: [], files: [],
      },
    })),
    cancelPersonalScriptRun: vi.fn(async () => undefined),
  };
}

describe('ScriptCenter', () => {
  it('imports disabled, enables deliberately, and still requires an unrecoverable preview', async () => {
    const user = userEvent.setup();
    const api = fixtureApi();
    render(<ScriptCenter apiClient={api} servers={[server]} serverId={server.id} />);

    const file = new File(['{"schemaVersion":1}'], 'script.json', { type: 'application/json' });
    if (!('text' in file)) Object.defineProperty(file, 'text', { value: async () => '{"schemaVersion":1}' });
    await user.upload(screen.getByLabelText('选择脚本包'), file);
    expect(await screen.findByText('已导入，默认未启用')).toBeVisible();
    expect(screen.getByRole('button', { name: '运行脚本' })).toBeDisabled();

    await user.click(screen.getByRole('button', { name: '启用脚本' }));
    expect(await screen.findByText('脚本已启用，运行时仍需二次确认。')).toBeVisible();
    await user.click(screen.getByRole('button', { name: '运行脚本' }));
    expect(await screen.findByText(/不可自动回滚/)).toBeVisible();
    expect(api.previewPersonalScriptRun).toHaveBeenCalledWith('script-1', 'server-1', {});
  });

  it('never writes an edited script body to browser storage or console', async () => {
    const user = userEvent.setup();
    const api = fixtureApi();
    const local = vi.spyOn(Storage.prototype, 'setItem');
    const consoleLog = vi.spyOn(console, 'log').mockImplementation(() => undefined);
    const { unmount } = render(<ScriptCenter apiClient={api} servers={[server]} serverId={server.id} />);

    await user.click(screen.getByRole('button', { name: '新建脚本' }));
    const body = screen.getByLabelText('Shell 脚本正文');
    await user.clear(body);
    await user.type(body, 'echo script-browser-canary');
    await user.type(screen.getByLabelText('脚本名称'), '浏览器泄漏检查');
    await user.click(screen.getByRole('button', { name: '创建脚本' }));
    await screen.findByText('脚本已创建，检查内容后请手动启用。');
    unmount();

    expect(api.createPersonalScript).toHaveBeenCalledWith(expect.objectContaining({ body: 'echo script-browser-canary' }));
    expect(JSON.stringify(local.mock.calls)).not.toContain('script-browser-canary');
    expect(JSON.stringify(consoleLog.mock.calls)).not.toContain('script-browser-canary');
    local.mockRestore();
    consoleLog.mockRestore();
  });

  it('keeps the list and editor as separate scroll regions', async () => {
    const api = fixtureApi(scriptDetails());
    const { container } = render(<ScriptCenter apiClient={api} servers={[server]} serverId={server.id} />);
    await waitFor(() => expect(api.listPersonalScripts).toHaveBeenCalled());
    expect(container.querySelector('.script-list-pane')).toBeInTheDocument();
    expect(container.querySelector('.script-editor__scroll')).toBeInTheDocument();
  });
});
