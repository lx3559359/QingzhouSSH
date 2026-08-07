import { ArrowRight, Database, FolderOpen, ShieldCheck, SpinnerGap, X } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import type { DataMigrationPreview, ReadyBootstrapStatus } from '../../api/contracts';
import { chooseDirectory } from '../../api/dialogs';
import { api, asAppError } from '../../api/tauri';

interface DataRootMigrationDialogProps {
  bootstrap: ReadyBootstrapStatus;
  intent?: 'choose' | 'retry' | 'portable_default';
  onClose: () => void;
}

export function DataRootMigrationDialog({ bootstrap, intent = 'choose', onClose }: DataRootMigrationDialogProps) {
  const [preview, setPreview] = useState<DataMigrationPreview | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState<'choosing' | 'starting' | null>(null);
  const [exiting, setExiting] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (intent === 'choose') return;
    let active = true;
    setBusy('choosing');
    const request = intent === 'retry'
      ? api.preflightRetryDataRootMigration()
      : api.preflightPortableDefaultDataRootMigration();
    void request
      .then((value) => { if (active) setPreview(value); })
      .catch((cause) => { if (active) setError(migrationErrorMessage(asAppError(cause).message, intent)); })
      .finally(() => { if (active) setBusy(null); });
    return () => { active = false; };
  }, [intent]);

  async function chooseTarget() {
    setError('');
    const target = await chooseDirectory({
      title: '选择新的轻舟 SSH 数据目录',
      previewPath: `${bootstrap.dataRoot}\\preview-migration-target`,
    });
    if (!target) return;
    setBusy('choosing');
    setPreview(null);
    setConfirmed(false);
    try {
      setPreview(await api.preflightDataRootMigration(target));
    } catch (cause) {
      setError(migrationErrorMessage(asAppError(cause).message, 'choose'));
    } finally {
      setBusy(null);
    }
  }

  async function startMigration() {
    if (!preview || !confirmed) return;
    setBusy('starting');
    setError('');
    try {
      await api.startDataRootMigration(preview.previewId, preview.confirmationToken);
      setExiting(true);
    } catch (cause) {
      setError(migrationErrorMessage(asAppError(cause).message, intent));
      setBusy(null);
    }
  }

  return (
    <div className="dialog-backdrop" role="presentation">
      <section className="silver-card modal-card data-migration-dialog" role="dialog" aria-modal="true" aria-labelledby="data-migration-title">
        <header className="data-migration-dialog__header">
          <span className="feature-icon feature-icon--blue"><Database weight="duotone" /></span>
          <div>
            <span className="eyebrow">安全迁移 · 逐文件校验 · 旧目录保留</span>
            <h2 id="data-migration-title">{intent === 'retry' ? '重试数据迁移' : '更改数据目录'}</h2>
            <p>客户端退出后复制并逐文件校验，完成后会自动重新打开。</p>
          </div>
          <button className="icon-button" type="button" aria-label="关闭数据目录迁移" disabled={busy !== null || exiting} onClick={onClose}><X /></button>
        </header>

        {exiting ? (
          <div className="data-migration-exiting" role="status">
            <SpinnerGap className="spin" weight="bold" />
            <div><strong>正在安全迁移数据，请等待客户端重新打开</strong><span>迁移期间不要移动源目录或目标目录。</span></div>
          </div>
        ) : (
          <>
            <div className="data-migration-current"><small>当前数据目录</small><strong>{bootstrap.dataRoot}</strong></div>
            {intent === 'choose' ? (
              <button className="secondary-button data-migration-choose" type="button" disabled={busy !== null} onClick={() => void chooseTarget()}>
                <FolderOpen weight="bold" />{busy === 'choosing' ? '正在预检目录…' : '选择新的空文件夹'}
              </button>
            ) : busy === 'choosing' ? (
              <div className="data-migration-preparing" role="status"><SpinnerGap className="spin" />正在核对迁移目录和剩余空间…</div>
            ) : null}

            {preview && (
              <>
                <div className="data-migration-paths">
                  <div><small>当前目录</small><strong>{preview.source}</strong></div>
                  <ArrowRight weight="bold" />
                  <div><small>目标目录</small><strong>{preview.target}</strong></div>
                </div>
                <dl className="data-migration-facts">
                  <div><dt>文件数量</dt><dd>{preview.fileCount} 个</dd></div>
                  <div><dt>数据大小</dt><dd>{formatBytes(preview.totalBytes)}</dd></div>
                  <div><dt>所需空间</dt><dd>{formatBytes(preview.requiredBytes)}</dd></div>
                  <div><dt>目标可用</dt><dd>{formatBytes(preview.availableBytes)}</dd></div>
                </dl>
                <div className="data-migration-retention"><ShieldCheck weight="fill" /><strong>旧目录不会删除或清空，迁移后仍由你自行保留和验证。</strong></div>
                {preview.retryable && <div className="data-migration-retry-note">只会补传缺失或校验不同的文件，不会删除目标目录中的数据。</div>}
                <label className="data-migration-confirm">
                  <input type="checkbox" checked={confirmed} onChange={(event) => setConfirmed(event.target.checked)} />
                  <span>我已理解：客户端会退出并重启，期间不要移动目录。</span>
                </label>
                <button className="success-button" type="button" disabled={!confirmed || busy !== null} onClick={() => void startMigration()}>
                  {busy === 'starting' ? <SpinnerGap className="spin" /> : <ShieldCheck weight="bold" />}
                  {busy === 'starting' ? '正在启动迁移…' : '确认迁移并退出'}
                </button>
              </>
            )}
          </>
        )}

        {error && <div className="inline-message inline-message--error" role="alert">{error}</div>}
      </section>
    </div>
  );
}

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
}

function migrationErrorMessage(message: string, intent: 'choose' | 'retry' | 'portable_default') {
  if (message.includes('不是空目录') || message.includes('覆盖')) {
    return intent === 'portable_default'
      ? '程序旁的 data 文件夹中已有旧数据。为避免覆盖，请先把该文件夹改名保留，再重新点击“恢复程序旁目录”。'
      : '目标文件夹中已有其他数据，请选择一个空文件夹。';
  }
  if (message.includes('磁盘空间不足')) return '目标磁盘空间不足，请清理空间或选择其他磁盘。';
  if (message.includes('父目录或子目录') || message.includes('相同')) return '新目录不能是当前目录本身、父目录或子目录。';
  if (message.includes('不可写') || message.includes('权限')) return '目标文件夹不可写，请选择你有权限使用的文件夹。';
  if (message.includes('重解析') || message.includes('链接')) return '为避免复制到目录之外，不能选择链接或重解析目录。';
  return message;
}
