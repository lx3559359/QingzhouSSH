import { DownloadSimple, File, SpinnerGap } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import type { ExecutionFile } from '../../api/contracts';
import { api } from '../../api/tauri';

function formatBytes(bytes: number) {
  if (bytes >= 1024 * 1024) return `${Number((bytes / (1024 * 1024)).toFixed(1))} MB`;
  if (bytes >= 1024) return `${Number((bytes / 1024).toFixed(1))} KB`;
  return `${bytes} B`;
}

export function DownloadsPage() {
  const [files, setFiles] = useState<ExecutionFile[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    api.listExecutions({})
      .then((records) => Promise.all(records.map((record) => api.getExecution(record.id))))
      .then((details) => {
        if (!active) return;
        const unique = new Map<string, ExecutionFile>();
        details.forEach((detail) => detail?.files.forEach((file) => unique.set(file.id, file)));
        setFiles([...unique.values()]);
      })
      .catch(() => active && setError('项目文件索引加载失败，请稍后重试。'))
      .finally(() => active && setLoading(false));
    return () => { active = false; };
  }, []);

  return (
    <section className="downloads-page" aria-labelledby="downloads-title">
      <header className="page-heading"><div><span className="eyebrow">后端索引 · 项目数据目录</span><h1 id="downloads-title">下载文件</h1><p>这里只展示轻舟后端登记过的相对路径，不允许 WebView 扫描任意磁盘位置。</p></div></header>
      {loading ? <div className="silver-card page-loading"><SpinnerGap className="spin" weight="bold" />正在读取文件索引…</div> : error ? <p className="inline-message inline-message--error" role="alert">{error}</p> : files.length === 0 ? <div className="silver-card log-empty-state"><DownloadSimple weight="duotone" /><strong>暂无项目文件</strong><span>日志结果和远端下载完成后会显示在这里。</span></div> : <div className="download-file-grid">{files.map((file) => <article className="silver-card download-file-card" key={file.id}><span className="feature-icon feature-icon--blue"><File weight="duotone" /></span><div><strong>{file.relativePath}</strong><small>{file.purpose} · {formatBytes(file.sizeBytes)}</small><code title={file.sha256}>SHA-256 {file.sha256}</code></div></article>)}</div>}
    </section>
  );
}
