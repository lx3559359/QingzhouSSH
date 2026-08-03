import type { LogMatch } from '../../api/contracts';

interface LogResultsTableProps {
  items: LogMatch[];
  searched: boolean;
}

export function LogResultsTable({ items, searched }: LogResultsTableProps) {
  if (searched && items.length === 0) {
    return (
      <div className="log-empty-state" role="status">
        <strong>没有找到匹配日志</strong>
        <span>可以缩短关键词、扩大时间范围或增加结果上限后重试。</span>
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
          {items.map((item, index) => (
            <tr key={`${item.path}:${item.lineNumber}:${index}`} className={`log-row--${item.kind}`}>
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
