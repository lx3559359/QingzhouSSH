import { GearSix, Lightning, Scroll, TerminalWindow } from '@phosphor-icons/react';

import type { ToolLibraryItem } from './types';

interface ToolCatalogListProps {
  items: ToolLibraryItem[];
  selectedKey: string;
  onSelect: (item: ToolLibraryItem) => void;
}

const STATE_LABELS = {
  ready: '可直接运行',
  remediable: '可补齐组件',
  permission_blocked: '权限受限',
  unsupported: '暂不支持',
} as const;

export function ToolCatalogList({ items, selectedKey, onSelect }: ToolCatalogListProps) {
  return (
    <section className="silver-card tool-catalog" aria-label="工具列表">
      <div className="tool-pane-heading">
        <span className="eyebrow">搜索结果</span>
        <h2>{items.length} 个工具</h2>
      </div>
      <div className="tool-catalog__scroll">
        {items.map((item) => (
          <button
            key={`${item.source}:${item.id}`}
            type="button"
            className={`tool-list-item ${selectedKey === `${item.source}:${item.id}` ? 'is-selected' : ''}`}
            aria-label={`选择任务 ${item.title}`}
            onClick={() => onSelect(item)}
          >
            <span className={`feature-icon ${item.category === 'service_management' ? 'feature-icon--orange' : item.source === 'personal_script' ? 'feature-icon--green' : 'feature-icon--blue'}`}>
              {item.source === 'personal_script' ? <Scroll weight="duotone" /> : item.category === 'service_management' ? <GearSix weight="duotone" /> : item.category === 'daily_inspection' ? <TerminalWindow weight="duotone" /> : <Lightning weight="duotone" />}
            </span>
            <span className="tool-list-item__copy">
              <h3>{item.title}</h3>
              <small>{item.description}</small>
              <span className={`availability-chip availability-chip--${item.state}`}>{STATE_LABELS[item.state]}</span>
            </span>
          </button>
        ))}
        {items.length === 0 && <div className="tool-empty-state">没有符合条件的工具，请调整分类或关键词。</div>}
      </div>
    </section>
  );
}
