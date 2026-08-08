import {
  ArrowsClockwise,
  CheckCircle,
  ClockCounterClockwise,
  Database,
  DownloadSimple,
  FolderOpen,
  GearSix,
  GithubLogo,
  HardDrive,
  ShieldWarning,
  ShieldCheck,
  Trash,
  WarningCircle,
  X,
} from '@phosphor-icons/react';
import { useCallback, useEffect, useState } from 'react';

import type { ReadyBootstrapStatus, UpdatePhase, UpdateProgressEvent, UpdateStatus } from '../../api/contracts';
import { api, asAppError } from '../../api/tauri';
import { DataRootMigrationDialog } from './DataRootMigrationDialog';

type BusyAction =
  | 'loading'
  | 'checking'
  | 'preference'
  | 'downloading'
  | 'installing'
  | 'clearing'
  | null;

const phaseLabels: Record<UpdatePhase, string> = {
  idle: '尚未检查',
  checking: '正在检查',
  up_to_date: '当前已是最新版本',
  available: '发现可用更新',
  downloading: '正在下载更新',
  downloaded: '更新已下载',
  installing: '正在启动安装',
  failed: '更新操作失败',
};

function formatBytes(bytes: number) {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function formatCheckedAt(value: number | null) {
  if (value === null) return '尚无检查记录';
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(value * 1000));
}

function statusTone(phase: UpdatePhase) {
  if (phase === 'failed') return 'failed';
  if (phase === 'available' || phase === 'downloaded') return 'available';
  if (phase === 'up_to_date') return 'ready';
  return 'neutral';
}

export function SettingsPage({ bootstrap }: { bootstrap: ReadyBootstrapStatus }) {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [busy, setBusy] = useState<BusyAction>('loading');
  const [error, setError] = useState('');
  const [progress, setProgress] = useState<UpdateProgressEvent | null>(null);
  const [confirmInstall, setConfirmInstall] = useState(false);
  const [migrationIntent, setMigrationIntent] = useState<'choose' | 'retry' | 'portable_default' | null>(null);

  const loadStatus = useCallback(async () => {
    setBusy('loading');
    setError('');
    try {
      setStatus(await api.getUpdateStatus());
    } catch (cause) {
      setError(asAppError(cause).message);
    } finally {
      setBusy(null);
    }
  }, []);

  useEffect(() => {
    void loadStatus();
  }, [loadStatus]);

  const checkForUpdates = async () => {
    setBusy('checking');
    setError('');
    setProgress(null);
    try {
      setStatus(await api.checkForUpdate(true));
    } catch (cause) {
      setError(asAppError(cause).message);
      try {
        setStatus(await api.getUpdateStatus());
      } catch {
        // Keep the last safe status already rendered.
      }
    } finally {
      setBusy(null);
    }
  };

  const changeAutoCheck = async (enabled: boolean) => {
    if (!status) return;
    const previous = status;
    setStatus({ ...status, autoCheck: enabled });
    setBusy('preference');
    setError('');
    try {
      setStatus(await api.setAutoUpdateCheck(enabled));
    } catch (cause) {
      setStatus(previous);
      setError(asAppError(cause).message);
    } finally {
      setBusy(null);
    }
  };

  const downloadUpdate = async () => {
    if (!status || status.phase !== 'available') return;
    setBusy('downloading');
    setError('');
    setProgress({ sequence: 0, downloadedBytes: 0, totalBytes: status.release?.size ?? null });
    setStatus({ ...status, phase: 'downloading' });
    try {
      setStatus(await api.downloadUpdate(setProgress));
    } catch (cause) {
      setError(asAppError(cause).message);
      try {
        setStatus(await api.getUpdateStatus());
      } catch {
        setStatus({ ...status, phase: 'failed', lastError: asAppError(cause).message });
      }
    } finally {
      setBusy(null);
    }
  };

  const installUpdate = async () => {
    setConfirmInstall(false);
    setBusy('installing');
    setError('');
    try {
      setStatus(await api.installUpdate(true));
    } catch (cause) {
      setError(asAppError(cause).message);
    } finally {
      setBusy(null);
    }
  };

  const clearUpdate = async () => {
    setBusy('clearing');
    setError('');
    try {
      setStatus(await api.clearDownloadedUpdate());
      setProgress(null);
    } catch (cause) {
      setError(asAppError(cause).message);
    } finally {
      setBusy(null);
    }
  };

  const progressPercent = status?.phase === 'downloaded'
    ? 100
    : progress?.totalBytes
      ? Math.min(100, Math.round((progress.downloadedBytes / progress.totalBytes) * 100))
      : 0;

  return (
    <section className="settings-page" aria-labelledby="settings-title">
      <header className="page-heading settings-heading">
        <div>
          <span className="eyebrow">本地设置 · 安全更新</span>
          <h1 id="settings-title">设置与更新</h1>
          <p>查看项目数据位置，管理更新检查，并从受信任的双源获取签名版本。</p>
        </div>
        <button
          className="secondary-button"
          type="button"
          disabled={!status || busy !== null}
          onClick={checkForUpdates}
        >
          <ArrowsClockwise className={busy === 'checking' ? 'spin' : ''} weight="bold" />
          {busy === 'checking' ? '正在检查' : '立即检查'}
        </button>
      </header>

      {error && (
        <div className="inline-message inline-message--error settings-alert" role="alert">
          <WarningCircle weight="fill" />
          <span>{error}</span>
          <button type="button" onClick={loadStatus}>重新读取</button>
        </div>
      )}

      {!status ? (
        <article className="silver-card settings-loading" aria-live="polite">
          <GearSix className="spin" weight="duotone" />
          <div>
            <strong>正在读取更新设置…</strong>
            <span>状态数据保存在项目数据目录内</span>
          </div>
        </article>
      ) : (
        <>
          <div className="settings-summary-grid">
            <article className="silver-card settings-summary-card">
              <span className="feature-icon feature-icon--purple"><HardDrive weight="duotone" /></span>
              <div>
                <span>当前版本</span>
                <strong>v{status.currentVersion}</strong>
                <small>当前客户端平台 · 当前用户安全边界</small>
              </div>
            </article>

            <article className="silver-card settings-summary-card settings-summary-card--path">
              <span className="feature-icon feature-icon--blue"><Database weight="duotone" /></span>
              <div>
                <span>数据目录</span>
                <strong title={bootstrap.dataRoot}>{bootstrap.dataRoot}</strong>
                <small>
                  {bootstrap.dataRootSource === 'environment'
                    ? '由 QINGZHOU_DATA_ROOT 环境变量锁定'
                    : '配置、日志、缓存与更新均位于此目录'}
                </small>
              </div>
              <button
                className="secondary-button data-root-change-button"
                type="button"
                disabled={!bootstrap.dataRootMutable}
                title={!bootstrap.dataRootMutable ? '环境变量锁定时不能在客户端修改' : undefined}
                onClick={() => setMigrationIntent('choose')}
              >
                <FolderOpen weight="bold" />更改数据目录
              </button>
              {bootstrap.dataRootSource === 'portable_custom' && (
                <button className="secondary-button data-root-restore-button" type="button" onClick={() => setMigrationIntent('portable_default')}>
                  恢复程序旁目录
                </button>
              )}
              {bootstrap.lastDataMigration?.phase === 'failed' && (
                <button className="secondary-button data-root-retry-button" type="button" onClick={() => setMigrationIntent('retry')}>
                  重试上次迁移
                </button>
              )}
            </article>
          </div>

          <div className="settings-main-grid">
            <article className="silver-card settings-panel update-preferences">
              <header className="settings-panel__header">
                <span className="feature-icon feature-icon--green"><ShieldCheck weight="duotone" /></span>
                <div>
                  <span className="eyebrow">更新策略</span>
                  <h2>可信双源检查</h2>
                </div>
              </header>

              <label className="update-toggle">
                <span>
                  <strong>自动检查更新</strong>
                  <small>最多每 6 小时检查一次，不会静默下载或安装。</small>
                </span>
                <input
                  type="checkbox"
                  aria-label="自动检查更新"
                  checked={status.autoCheck}
                  disabled={busy === 'preference'}
                  onChange={(event) => void changeAutoCheck(event.target.checked)}
                />
                <i aria-hidden="true" />
              </label>

              <dl className="update-facts">
                <div>
                  <dt>更新来源</dt>
                  <dd>
                    {status.release?.source === 'github' && <GithubLogo weight="fill" />}
                    {status.release?.sourceLabel ?? 'GitHub 主源 · ModelScope 回退'}
                  </dd>
                </div>
                <div>
                  <dt>上次检查</dt>
                  <dd><ClockCounterClockwise weight="duotone" />{formatCheckedAt(status.lastCheckedAt)}</dd>
                </div>
                <div>
                  <dt>安全边界</dt>
                  <dd><ShieldCheck weight="fill" />签名与 SHA-256 双重校验</dd>
                </div>
              </dl>
            </article>

            <article className="silver-card settings-panel update-status-panel">
              <header className="settings-panel__header">
                <span className={`update-state-icon update-state-icon--${statusTone(status.phase)}`}>
                  {status.phase === 'failed'
                    ? <WarningCircle weight="fill" />
                    : <CheckCircle weight="duotone" />}
                </span>
                <div>
                  <span className="eyebrow">最新状态</span>
                  <h2>
                    {status.phase === 'failed'
                      ? phaseLabels.failed
                      : status.lastResult?.message ?? phaseLabels[status.phase]}
                  </h2>
                </div>
              </header>

              {status.fallbackReason && (
                <div className="update-fallback-note" role="status">
                  <ArrowsClockwise weight="bold" />
                  <span>{status.fallbackReason}</span>
                </div>
              )}

              {status.release ? (
                <section className="update-release" aria-labelledby="release-version">
                  <div className="update-release__title">
                    <div>
                      <span>可用版本</span>
                      <h3 id="release-version">版本 {status.release.version}</h3>
                    </div>
                    <span className="update-size">{formatBytes(status.release.size)}</span>
                  </div>
                  <p>{status.release.notes || '本版本未提供更新说明。'}</p>
                  <div className="update-release__meta">
                    <span>来源 · {status.release.sourceLabel}</span>
                    <span>构建 · {status.release.buildId}</span>
                  </div>

                  {(status.phase === 'downloading' || status.phase === 'downloaded') && (
                    <div className="update-download-progress" aria-live="polite">
                      <div
                        role="progressbar"
                        aria-label="更新下载进度"
                        aria-valuemin={0}
                        aria-valuemax={100}
                        aria-valuenow={progressPercent}
                      >
                        <i style={{ width: `${progressPercent}%` }} />
                      </div>
                      <span>{progressPercent}%</span>
                    </div>
                  )}

                  <div className="update-actions">
                    {status.phase === 'available' && (
                      <button
                        className="primary-button"
                        type="button"
                        disabled={busy !== null}
                        onClick={() => void downloadUpdate()}
                      >
                        <DownloadSimple weight="bold" />下载并验证
                      </button>
                    )}
                    {status.phase === 'downloading' && (
                      <button className="primary-button" type="button" disabled>
                        <ArrowsClockwise className="spin" weight="bold" />正在下载并校验
                      </button>
                    )}
                    {status.phase === 'downloaded' && (
                      <button
                        className="success-button"
                        type="button"
                        disabled={busy !== null}
                        onClick={() => setConfirmInstall(true)}
                      >
                        <ShieldCheck weight="bold" />安装更新
                      </button>
                    )}
                    {(status.phase === 'downloaded' || status.phase === 'failed') && (
                      <button
                        className="secondary-button"
                        type="button"
                        disabled={busy !== null}
                        onClick={() => void clearUpdate()}
                      >
                        <Trash weight="bold" />
                        {busy === 'clearing' ? '正在清理' : '清理更新文件'}
                      </button>
                    )}
                  </div>
                </section>
              ) : (
                <div className="update-empty">
                  <GearSix weight="duotone" />
                  <span>{phaseLabels[status.phase]}</span>
                </div>
              )}
            </article>
          </div>
        </>
      )}

      {confirmInstall && status?.release && (
        <div className="dialog-backdrop" role="presentation">
          <section
            className="silver-card modal-card update-install-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="update-install-title"
          >
            <header className="modal-header">
              <div className="update-install-dialog__heading">
                <ShieldWarning weight="fill" />
                <div>
                  <h2 id="update-install-title">确认安装更新</h2>
                  <p>请先保存正在查看的信息。</p>
                </div>
              </div>
              <button
                className="icon-button"
                type="button"
                aria-label="关闭安装确认"
                onClick={() => setConfirmInstall(false)}
              >
                <X weight="bold" />
              </button>
            </header>
            <div className="update-install-warning">
              <strong>轻舟 SSH 将退出并启动已验证的安装程序。</strong>
              <span>版本 {status.release.version} 将按当前用户安装，不需要管理员权限；程序不会静默执行。</span>
            </div>
            <div className="modal-actions">
              <button className="secondary-button" type="button" onClick={() => setConfirmInstall(false)}>
                暂不安装
              </button>
              <button className="danger-button" type="button" onClick={() => void installUpdate()}>
                确认安装并退出
              </button>
            </div>
          </section>
        </div>
      )}

      {migrationIntent && (
        <DataRootMigrationDialog bootstrap={bootstrap} intent={migrationIntent} onClose={() => setMigrationIntent(null)} />
      )}
    </section>
  );
}
