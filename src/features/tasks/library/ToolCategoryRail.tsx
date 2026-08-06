import type { ToolLibraryGroupCounts, UnifiedToolCategory } from './types';

const LABELS: Record<UnifiedToolCategory, string> = {
  recommended_recent: '推荐与最近',
  daily_inspection: '日常巡检',
  performance: '性能与卡顿',
  storage: '磁盘与存储',
  network: '网络与端口',
  web_service: '网站与应用',
  security_login: '安全与登录',
  service_management: '服务管理',
  container: '容器',
  system_settings: '系统设置',
  my_scripts: '我的脚本',
};

interface ToolCategoryRailProps {
  selected: UnifiedToolCategory | 'all';
  counts: ToolLibraryGroupCounts;
  total: number;
  onSelect: (category: UnifiedToolCategory | 'all') => void;
}

export function ToolCategoryRail({ selected, counts, total, onSelect }: ToolCategoryRailProps) {
  return (
    <nav className="silver-card tool-category-rail" aria-label="工具分类">
      <div className="tool-pane-heading">
        <span className="eyebrow">快速定位</span>
        <h2>工具分类</h2>
      </div>
      <button type="button" className={selected === 'all' ? 'is-active' : ''} onClick={() => onSelect('all')}>
        <span>全部工具</span><b>{total}</b>
      </button>
      {(Object.keys(LABELS) as UnifiedToolCategory[])
        .filter((category) => counts[category] > 0)
        .map((category) => (
          <button key={category} type="button" className={selected === category ? 'is-active' : ''} onClick={() => onSelect(category)}>
            <span>{LABELS[category]}</span><b>{counts[category]}</b>
          </button>
        ))}
    </nav>
  );
}
