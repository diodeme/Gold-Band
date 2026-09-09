import { resolve } from 'node:path';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { cp } from 'node:fs/promises';
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
      await cp(resolve(process.env.SITE_WORKFLOW_ASSETS || 'marketing/site/media/workflow'), resolve(options.dir!, 'media/workflow'), { recursive: true });
      await cp(resolve(process.env.SITE_BEFORE_ASSETS || 'marketing/site/media/before'), resolve(options.dir!, 'media/before'), { recursive: true });
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
