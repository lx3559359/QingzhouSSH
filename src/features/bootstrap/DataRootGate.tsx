import { FolderOpen, HardDrives } from '@phosphor-icons/react';
import { useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';

import { api } from '../../api/tauri';
import type { BootstrapStatus } from '../../api/contracts';

type ReadyStatus = Extract<BootstrapStatus, { state: 'ready' }>;

interface DataRootGateProps {
  status: BootstrapStatus;
  onReady: (status: ReadyStatus) => void;
}

export function DataRootGate({ status, onReady }: DataRootGateProps) {
  const [selectedPath, setSelectedPath] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  async function chooseDirectory() {
    setError('');
    const path = await open({
      directory: true,
      multiple: false,
      title: '选择轻舟 SSH 数据目录',
    });
    if (!path) return;

    setSelectedPath(path);
    setBusy(true);
    try {
      const ready = await api.initializeDataRoot(path);
      if (ready.state !== 'ready') {
        throw new Error('数据目录初始化未完成');
      }
      onReady(ready);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  }

  if (status.state === 'ready') return null;

  return (
    <section className="bootstrap-shell">
      <article className="silver-card data-root-card" aria-labelledby="data-root-title">
        <div className="feature-icon feature-icon--blue" aria-hidden="true">
          <HardDrives weight="duotone" />
        </div>
        <div className="card-heading">
          <span className="eyebrow">首次启动 · 本地存储</span>
          <h1 id="data-root-title">选择数据存储位置</h1>
          <p>数据库、凭据密文、日志、下载和更新文件都将保存在这里。</p>
        </div>

        <div className="storage-note">
          <strong>由你决定保存位置</strong>
          <span>应用不会自动使用系统默认数据目录。</span>
        </div>

        {selectedPath && (
          <output className="selected-path" aria-live="polite">
            {selectedPath}
          </output>
        )}
        {error && (
          <p className="inline-message inline-message--error" role="alert">
            {error}
          </p>
        )}

        <button
          className="primary-button"
          type="button"
          disabled={busy}
          onClick={chooseDirectory}
        >
          <FolderOpen weight="bold" aria-hidden="true" />
          {busy ? '正在初始化…' : '选择文件夹'}
        </button>
      </article>
    </section>
  );
}
