import { open } from '@tauri-apps/plugin-dialog';

interface ChooseDirectoryOptions {
  title: string;
  previewPath: string;
}

export async function chooseDirectory({ title, previewPath }: ChooseDirectoryOptions) {
  if (isBrowserPreview()) return previewPath || null;

  const selected = await open({ directory: true, multiple: false, title });
  return typeof selected === 'string' ? selected : null;
}

function isBrowserPreview() {
  if (!import.meta.env.DEV || typeof window === 'undefined') return false;
  const parameters = new URLSearchParams(window.location.search);
  const preview = parameters.get('preview');
  if (preview === 'ready' || preview === 'data-root') return true;
  return ['github', 'modelscope', 'reject', 'up_to_date'].includes(
    parameters.get('update') ?? '',
  );
}
