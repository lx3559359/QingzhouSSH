import { Minus, Square, X } from '@phosphor-icons/react';

import { windowControls } from './nativeWindow';

function runWindowAction(action: () => Promise<void>) {
  void action().catch(() => undefined);
}

export function WindowControls() {
  return (
    <div className="window-controls" aria-label="窗口控制">
      <button
        type="button"
        aria-label="最小化窗口"
        title="最小化"
        onClick={() => runWindowAction(windowControls.minimize)}
      >
        <Minus weight="bold" aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label="最大化或还原窗口"
        title="最大化或还原"
        onClick={() => runWindowAction(windowControls.toggleMaximize)}
      >
        <Square weight="bold" aria-hidden="true" />
      </button>
      <button
        className="window-controls__close"
        type="button"
        aria-label="关闭窗口"
        title="关闭"
        onClick={() => runWindowAction(windowControls.close)}
      >
        <X weight="bold" aria-hidden="true" />
      </button>
    </div>
  );
}
