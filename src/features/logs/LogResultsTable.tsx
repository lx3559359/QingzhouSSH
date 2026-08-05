import type { SearchResultItem } from '../../api/contracts';
import type { KeyboardEvent, MouseEvent } from 'react';

interface LogResultsTableProps {
  items: SearchResultItem[];
  searched: boolean;
  onItemContextMenu?: (item: SearchResultItem, position: { x: number; y: number }) => void;
}

function formatBytes(bytes: number | null) {
  if (bytes == null) return '未知';
  if (bytes >= 1024 * 1024) return `${Number((bytes / (1024 * 1024)).toFixed(1))} MB`;
  if (bytes >= 1024) return `${Number((bytes / 1024).toFixed(1))} KB`;
  return `${bytes} B`;
}

function formatModified(seconds: number | null) {
  if (seconds == null) return '未知';
  return new Date(seconds * 1000).toLocaleString('zh-CN', { hour12: false });
}

export function LogResultsTable({ items, searched, onItemContextMenu }: LogResultsTableProps) {
  const openContextMenu = (
    item: SearchResultItem,
    event: MouseEvent<HTMLTableRowElement> | KeyboardEvent<HTMLTableRowElement>,
  ) => {
    if (!onItemContextMenu) return;
    event.preventDefault();
    if ('clientX' in event && (event.clientX || event.clientY)) {
      onItemContextMenu(item, { x: event.clientX, y: event.clientY });
      return;
    }
    const bounds = event.currentTarget.getBoundingClientRect();
    onItemContextMenu(item, { x: bounds.left + 20, y: bounds.top + 20 });
  };

  const onRowKeyDown = (item: SearchResultItem, event: KeyboardEvent<HTMLTableRowElement>) => {
    if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      openContextMenu(item, event);
    }
  };
  if (searched && items.length === 0) {
    return (
      <div className="log-empty-state" role="status">
        <strong>没有找到匹配结果</strong>
        <span>可以缩短关键词后重试。</span>
      </div>
    );
  }

  const filenameResults = items.every((item) => item.resultType === 'file');

  if (filenameResults) {
    return (
      <div className="log-table-wrap">
        <table className="log-results-table log-file-results-table">
          <thead>
            <tr>
              <th scope="col">文件名</th>
              <th scope="col">完整路径</th>
              <th scope="col">大小</th>
              <th scope="col">修改时间</th>
            </tr>
          </thead>
          <tbody>
            {items.map((item) => item.resultType === 'file' && (
              <tr
                key={item.path}
                tabIndex={onItemContextMenu ? 0 : undefined}
                onContextMenu={(event) => openContextMenu(item, event)}
                onKeyDown={(event) => onRowKeyDown(item, event)}
              >
                <td><strong>{item.name}</strong></td>
                <td><code title={item.path}>{item.path}</code></td>
                <td>{formatBytes(item.size)}</td>
                <td>{formatModified(item.modifiedAt)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  }

  if (!searched) {
    return (
      <div className="log-empty-state log-empty-state--idle">
        <strong>等待检索</strong>
        <span>结果会在远端完成检索后，以每页 50 条显示在这里。</span>
      </div>
    );
  }

  return (
    <div className="log-table-wrap">
      <table className="log-results-table">
        <thead>
          <tr>
            <th scope="col">位置</th>
            <th scope="col">时间</th>
            <th scope="col">类型</th>
            <th scope="col">内容</th>
          </tr>
        </thead>
        <tbody>
          {items.map((item, index) => item.resultType === 'content' && (
            <tr
              key={`${item.path}:${item.lineNumber}:${index}`}
              className={`log-row--${item.kind}`}
              tabIndex={onItemContextMenu ? 0 : undefined}
              onContextMenu={(event) => openContextMenu(item, event)}
              onKeyDown={(event) => onRowKeyDown(item, event)}
            >
              <td><span title={item.path}>{item.path}</span><small>第 {item.lineNumber} 行</small></td>
              <td>{item.timestamp ?? '—'}</td>
              <td><span className={`log-kind log-kind--${item.kind}`}>{item.kind === 'match' ? '匹配' : '上下文'}</span></td>
              <td><code>{item.text}</code></td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
