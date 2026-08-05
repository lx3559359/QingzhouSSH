import { Copy, FileCode, Star } from '@phosphor-icons/react';

import type { PersonalScriptSummary } from '../../../api/contracts';

interface ScriptListProps {
  scripts: PersonalScriptSummary[];
  selectedId: string | null;
  loading: boolean;
  onSelect: (scriptId: string) => void;
  onCopy: (scriptId: string) => void;
}

export function ScriptList({ scripts, selectedId, loading, onSelect, onCopy }: ScriptListProps) {
  if (loading) return <div className="script-list-empty" role="status">正在读取脚本列表…</div>;
  if (scripts.length === 0) {
    return (
      <div className="script-list-empty">
        <FileCode weight="duotone" />
        <strong>还没有个人脚本</strong>
        <span>可以新建，也可以导入固定格式的脚本包。</span>
      </div>
    );
  }
  return (
    <div className="script-list" role="list" aria-label="个人脚本列表">
      {scripts.map((script) => (
        <article
          className={`script-list-item ${selectedId === script.id ? 'is-selected' : ''}`}
          key={script.id}
          role="listitem"
        >
          <button type="button" className="script-list-item__main" onClick={() => onSelect(script.id)}>
            <span className="script-list-item__title">
              <strong>{script.title}</strong>
              {script.isFavorite && <Star weight="fill" aria-label="已收藏" />}
            </span>
            <span>{script.category} · v{script.activeVersionNumber}</span>
            <span className={`script-state ${script.isEnabled ? 'is-enabled' : ''}`}>
              {script.isEnabled ? '已启用' : '未启用'}
            </span>
          </button>
          <button
            type="button"
            className="script-list-item__copy"
            aria-label={`复制脚本 ${script.title}`}
            onClick={() => onCopy(script.id)}
          >
            <Copy />
          </button>
        </article>
      ))}
    </div>
  );
}
