import { MagnifyingGlass } from '@phosphor-icons/react';

interface ToolLibraryFiltersProps {
  query: string;
  showUnavailable: boolean;
  onQueryChange: (query: string) => void;
  onToggleUnavailable: () => void;
}

export function ToolLibraryFilters({
  query,
  showUnavailable,
  onQueryChange,
  onToggleUnavailable,
}: ToolLibraryFiltersProps) {
  return (
    <div className="tool-library-filters">
      <label className="tool-search-field">
        <MagnifyingGlass weight="bold" />
        <input
          aria-label="搜索工具"
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
          placeholder="例如：服务器很慢、端口被占用"
        />
      </label>
      <button type="button" className={showUnavailable ? 'is-active' : ''} onClick={onToggleUnavailable}>
        {showUnavailable ? '只看可用工具' : '查看受限与不支持'}
      </button>
    </div>
  );
}
