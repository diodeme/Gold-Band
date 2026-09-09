import { resolve } from 'node:path';
import { defineConfig, normalizePath } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { identitySensitiveDependencies } from '../../web/config/identity-sensitive-dependencies';
import { cpSync } from 'node:fs';
import { checkDataset } from './build-dataset.mjs';

export default defineConfig({
  base: process.env.VITE_DEMO_BASE ?? '/',
  root: 'marketing/demo',
  publicDir: '../../web/public',
  plugins: [
    {
      name: 'demo-history-dataset',
      apply: 'build',
      buildStart() {
        checkDataset(process.env.DEMO_DATASET_PATH ?? resolve('marketing/demo/data/ji-history'), process.env.DEMO_ALLOW_INCOMPLETE_DATASET === '1');
      },
      closeBundle() {
        const source = process.env.DEMO_DATASET_PATH ?? resolve('marketing/demo/data/ji-history');
        cpSync(source, resolve(process.env.DEMO_DIST ?? '.codex-temp/demo-dist', 'data/ji-history'), { recursive: true });
      },
    },
    {
      name: 'demo-runtime',
      enforce: 'pre',
      resolveId(source, importer) {
        if (importer?.replaceAll('\\', '/').endsWith('/web/src/api/client.ts')
          && (source === './browser' || source === './desktop')) {
          return normalizePath(resolve('marketing/demo/runtime.ts'));
        }
      },
    },
    react(), tailwindcss(),
  ],
  resolve: { alias: { '@': resolve('web/src') }, dedupe: [...identitySensitiveDependencies] },
  build: { target: 'safari15.4', outDir: process.env.DEMO_DIST ?? '../../.codex-temp/demo-dist', emptyOutDir: true },
});
