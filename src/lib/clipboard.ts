export async function copyText(value: string) {
  if (!navigator.clipboard?.writeText) {
    throw new Error('当前系统不支持复制到剪贴板');
  }
  await navigator.clipboard.writeText(value);
}
