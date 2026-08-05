import { getCurrentWindow } from '@tauri-apps/api/window';

const currentWindow = () => getCurrentWindow();

export const windowControls = {
  startDragging: () => currentWindow().startDragging(),
  minimize: () => currentWindow().minimize(),
  toggleMaximize: () => currentWindow().toggleMaximize(),
  close: () => currentWindow().close(),
};
