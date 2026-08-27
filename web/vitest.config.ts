import { defineConfig } from 'vitest/config';
import path from 'node:path';
import { identitySensitiveDependencies } from './config/identity-sensitive-dependencies';

export default defineConfig({
  resolve: {
    dedupe: [...identitySensitiveDependencies],
    alias: {
      '@': path.resolve(__dirname, 'src'),
    },
  },
  test: {
    include: ['web/tests/**/*.test.{ts,tsx}'],
    environment: 'node',
    server: {
      deps: {
        inline: ['@atomic-editor/editor', '@codemirror/lang-markdown', /^@codemirror\//, /^@lezer\//],
      },
    },
  },
});
