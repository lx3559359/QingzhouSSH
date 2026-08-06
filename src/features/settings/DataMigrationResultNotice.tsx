import { CheckCircle, FolderOpen, WarningCircle, X } from '@phosphor-icons/react';
import { useState } from 'react';

import type { DataMigrationJournal } from '../../api/contracts';
import { api, asAppError } from '../../api/tauri';

interface DataMigrationResultNoticeProps {
  journal: DataMigrationJournal | null;
  onRetry: () => void;
}

export function DataMigrationResultNotice({ journal, onRetry }: DataMigrationResultNoticeProps) {
  const [visible, setVisible] = useState(Boolean(journal && !journal.acknowledged));
  const [error, setError] = useState('');

  if (!visible || !journal || journal.acknowledged) return null;
  const migration = journal;
  const completed = migration.phase === 'completed';
  const failed = migration.phase === 'failed';
  if (!completed && !failed) return null;

  async function acknowledge() {
    setError('');
    try {
      await api.acknowledgeDataRootMigration(migration.migrationId);
      setVisible(false);
    } catch (cause) {
      setError(asAppError(cause).message);
    }
  }

  async function openOldDirectory() {
    setError('');
    try {
      await api.openDataRootFolder('last_source');
    } catch (cause) {
      setError(asAppError(cause).message);
    }
  }

  return (
    <aside className={`migration-result-notice migration-result-notice--${completed ? 'success' : 'failed'}`} role={failed ? 'alert' : 'status'}>
      <span className="migration-result-notice__icon">
        {completed ? <CheckCircle weight="fill" /> : <WarningCircle weight="fill" />}
      </span>
      <div className="migration-result-notice__body">
        <strong>{completed ? '数据目录迁移完成' : '数据目录迁移失败，原目录仍在使用'}</strong>
        <span>
          {completed
            ? `新目录已启用；旧目录仍完整保留在 ${migration.source}`
            : noviceFailureMessage(migration.errorSummary)}
        </span>
        {error && <small role="alert">{error}</small>}
      </div>
      <div className="migration-result-notice__actions">
        {completed ? (
          <button className="secondary-button" type="button" onClick={() => void openOldDirectory()}>
            <FolderOpen weight="bold" />打开旧目录
          </button>
        ) : (
          <button className="secondary-button" type="button" onClick={onRetry}>前往设置重试</button>
        )}
        <button className="icon-button" type="button" aria-label="关闭迁移结果" onClick={() => void acknowledge()}><X weight="bold" /></button>
      </div>
    </aside>
  );
}

function noviceFailureMessage(error: string | null) {
  if (!error) return '迁移没有更改数据目录，可在设置中重新检查并重试。';
  if (error.includes('磁盘') || error.includes('空间')) return '目标磁盘空间不足或不可用。清理空间后可在设置中安全重试。';
  if (error.includes('权限') || error.includes('不可写')) return '目标文件夹当前无法写入。检查权限后可在设置中安全重试。';
  if (error.includes('校验') || error.includes('完整')) return '复制后的文件校验未通过，因此没有切换目录。可在设置中安全重试。';
  return `未切换数据目录：${error}`;
}
