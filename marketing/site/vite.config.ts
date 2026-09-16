import { resolve } from 'node:path';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  root: 'marketing/site',
  publicDir: '../../web/public',
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': resolve('web/src') } },
  server: { strictPort: true },
  build: {
    target: 'safari15.4',
    outDir: '../../.codex-temp/site-dist', emptyOutDir: true,
    rollupOptions: { input: { site: resolve('marketing/site/index.html'), preview: resolve('marketing/site/preview.html') } },
  },
});
