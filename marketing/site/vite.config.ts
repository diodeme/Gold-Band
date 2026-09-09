import { resolve } from 'node:path';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { publishSceneAssets } from '../../scripts/site-publish-assets.mjs';
import { loadEnv } from 'vite';
import { siteConfig } from './config';

export default defineConfig(({ command, mode }) => {
  const config = siteConfig({ ...loadEnv(mode, process.cwd(), ''), ...process.env }, command === 'build');
  return {
  base: config.base,
  root: 'marketing/site',
  publicDir: '../../web/public',
  plugins: [react(), tailwindcss(), {
    name: 'workflow-recording-assets',
    async writeBundle(options) {
      for (const directory of ['workflow', 'before', 'after', 'personalize']) {
        await publishSceneAssets({ sourceRoot: resolve(process.env[`SITE_${directory.toUpperCase()}_ASSETS`] || `marketing/site/media/${directory}`), outputRoot: resolve(options.dir!, `media/${directory}`), directory, base: config.base });
      }
    },
  }],
  resolve: { alias: { '@': resolve('web/src') } },
  server: { strictPort: true },
  build: {
    target: 'safari15.4',
    outDir: '../../.codex-temp/site-dist', emptyOutDir: true,
    rollupOptions: { input: { site: resolve('marketing/site/index.html') } },
  },
}; });
