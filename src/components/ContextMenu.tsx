import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

export interface ContextMenuItem {
  id: string;
  label: string;
  onSelect: () => void | Promise<void>;
  disabled?: boolean;
  disabledReason?: string;
  separatorBefore?: boolean;
}

interface ContextMenuProps {
  position: { x: number; y: number };
  items: ContextMenuItem[];
  onClose: () => void;
}

const VIEWPORT_MARGIN = 8;

export function ContextMenu({ position, items, onClose }: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [renderPosition, setRenderPosition] = useState(position);

  useLayoutEffect(() => {
    const menu = menuRef.current;
    if (!menu) return;
    const bounds = menu.getBoundingClientRect();
    setRenderPosition({
      x: Math.max(VIEWPORT_MARGIN, Math.min(position.x, window.innerWidth - bounds.width - VIEWPORT_MARGIN)),
      y: Math.max(VIEWPORT_MARGIN, Math.min(position.y, window.innerHeight - bounds.height - VIEWPORT_MARGIN)),
    });
    menu.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
  }, [position, items]);

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) onClose();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
      const enabled = Array.from(
        menuRef.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [],
      );
      if (enabled.length === 0) return;
      event.preventDefault();
      const current = enabled.indexOf(document.activeElement as HTMLButtonElement);
      const movement = event.key === 'ArrowDown' ? 1 : -1;
      enabled[(current + movement + enabled.length) % enabled.length]?.focus();
    };
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [onClose]);

  return createPortal(
    <div
      ref={menuRef}
      className="desktop-context-menu"
      role="menu"
      aria-label="快捷操作"
      style={{ left: renderPosition.x, top: renderPosition.y }}
    >
      {items.map((item) => (
        <button
          key={item.id}
          className={item.separatorBefore ? 'has-separator' : undefined}
          type="button"
          role="menuitem"
          disabled={item.disabled}
          aria-label={item.disabled && item.disabledReason ? `${item.label}（${item.disabledReason}）` : item.label}
          title={item.disabledReason}
          onClick={async () => {
            await item.onSelect();
            onClose();
          }}
        >
          <span>{item.label}</span>
          {item.disabled && item.disabledReason && <small>{item.disabledReason}</small>}
        </button>
      ))}
    </div>,
    document.body,
  );
}
