import { defineConfig } from 'vitest/config';
import path from 'node:path';

export default defineConfig({
  resolve: {
    alias: {
      '@': path.resolve(__dirname, 'src'),
    },
  },
  test: {
    include: ['web/tests/**/*.test.{ts,tsx}'],
    environment: 'node',
    server: {
      deps: {
        inline: ['@atomic-editor/editor'],
      },
    },
  },
});
