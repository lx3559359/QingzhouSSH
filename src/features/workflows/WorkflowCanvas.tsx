import { Minus, Plus } from '@phosphor-icons/react';
import { useRef } from 'react';

import type { WorkflowDraft, WorkflowNode } from '../../api/contracts';

type DragState = {
  nodeId: string;
  startX: number;
  startY: number;
  originX: number;
  originY: number;
};

const nodeHints: Record<WorkflowNode['config']['type'], string> = {
  start: '流程入口',
  task: '受控任务',
  custom: '高级操作',
  upload: '发送到远端',
  download: '保存到本地数据目录',
  logSearch: '远端日志匹配',
  condition: 'true / false',
  stop: '流程终点',
};

export function WorkflowCanvas({
  draft,
  selectedNodeId,
  zoom,
  onSelect,
  onMove,
  onZoom,
}: {
  draft: WorkflowDraft;
  selectedNodeId: string | null;
  zoom: number;
  onSelect: (nodeId: string) => void;
  onMove: (nodeId: string, position: { x: number; y: number }) => void;
  onZoom: (zoom: number) => void;
}) {
  const drag = useRef<DragState | null>(null);
  const nodeById = new Map(draft.nodes.map((node) => [node.id, node]));

  const handlePointerDown = (event: React.PointerEvent<HTMLButtonElement>, node: WorkflowNode) => {
    drag.current = {
      nodeId: node.id,
      startX: event.clientX,
      startY: event.clientY,
      originX: node.position.x,
      originY: node.position.y,
    };
    event.currentTarget.setPointerCapture?.(event.pointerId);
    onSelect(node.id);
  };

  const handlePointerMove = (event: React.PointerEvent<HTMLButtonElement>) => {
    if (!drag.current || drag.current.nodeId !== event.currentTarget.dataset.nodeId) return;
    onMove(drag.current.nodeId, {
      x: Math.max(16, Math.round(drag.current.originX + (event.clientX - drag.current.startX) / zoom)),
      y: Math.max(16, Math.round(drag.current.originY + (event.clientY - drag.current.startY) / zoom)),
    });
  };

  return (
    <section className="silver-card workflow-canvas" aria-label="工作流画布">
      <header className="workflow-canvas__toolbar">
        <div>
          <span className="workflow-panel-kicker">可视化画布</span>
          <strong>{draft.name}</strong>
        </div>
        <div className="workflow-zoom" aria-label="画布缩放">
          <button type="button" aria-label="缩小画布" onClick={() => onZoom(Math.max(0.6, zoom - 0.1))}>
            <Minus aria-hidden="true" />
          </button>
          <span>{Math.round(zoom * 100)}%</span>
          <button type="button" aria-label="放大画布" onClick={() => onZoom(Math.min(1.4, zoom + 0.1))}>
            <Plus aria-hidden="true" />
          </button>
        </div>
      </header>
      <div className="workflow-canvas__viewport">
        <div className="workflow-canvas__board" style={{ transform: `scale(${zoom})` }}>
          <svg className="workflow-edges" viewBox="0 0 1000 620" aria-label="工作流连接">
            <defs>
              <marker id="workflow-arrow" markerWidth="9" markerHeight="9" refX="7" refY="4" orient="auto">
                <path d="M0,0 L0,8 L8,4 z" />
              </marker>
            </defs>
            {draft.edges.map((edge) => {
              const from = nodeById.get(edge.from);
              const to = nodeById.get(edge.to);
              if (!from || !to) return null;
              const x1 = from.position.x + 150;
              const y1 = from.position.y + 40;
              const x2 = to.position.x;
              const y2 = to.position.y + 40;
              const bend = Math.max(42, Math.abs(x2 - x1) * 0.5);
              return (
                <path
                  key={`${edge.from}-${edge.to}-${edge.branch}`}
                  className={`workflow-edge workflow-edge--${edge.branch}`}
                  d={`M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`}
                  markerEnd="url(#workflow-arrow)"
                />
              );
            })}
          </svg>
          {draft.nodes.map((node) => (
            <button
              type="button"
              key={node.id}
              data-node-id={node.id}
              aria-pressed={selectedNodeId === node.id}
              className={`workflow-node workflow-node--${node.config.type}${selectedNodeId === node.id ? ' is-selected' : ''}`}
              style={{ left: `${node.position.x}px`, top: `${node.position.y}px` }}
              onPointerDown={(event) => handlePointerDown(event, node)}
              onPointerMove={handlePointerMove}
              onPointerUp={() => { drag.current = null; }}
              onPointerCancel={() => { drag.current = null; }}
              onClick={() => onSelect(node.id)}
            >
              <span className="workflow-node__glyph" aria-hidden="true" />
              <span>
                <strong>{node.name}</strong>
                <small>{nodeHints[node.config.type]}</small>
              </span>
            </button>
          ))}
        </div>
      </div>
    </section>
  );
}
