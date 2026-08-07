import react from '@vitejs/plugin-react';
import { configDefaults, defineConfig } from 'vitest/config';
import packageMetadata from './package.json';

export default defineConfig({
  plugins: [react()],
  define: {
    __APP_VERSION__: JSON.stringify(packageMetadata.version),
  },
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ['**/.local/**', '**/target/**', '**/artifacts/**', '**/.worktrees/**'],
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    testTimeout: 15_000,
    hookTimeout: 15_000,
    exclude: [...configDefaults.exclude, '**/.worktrees/**', '**/.local/**', '**/target/**'],
  },
});
