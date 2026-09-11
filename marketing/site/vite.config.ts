import { resolve } from 'node:path';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

const base = process.env.WEBSITE_BASE ?? '/';
const outDir = process.env.WEBSITE_SITE_DIR ? resolve(process.env.WEBSITE_SITE_DIR) : '../../.codex-temp/site-dist';

export default defineConfig({
  root: 'marketing/site',
  base,
  publicDir: '../../web/public',
  plugins: [react(), tailwindcss()],
  resolve: { alias: { '@': resolve('web/src') } },
  server: { strictPort: true },
  build: {
    target: 'safari15.4',
    outDir, emptyOutDir: true,
    rollupOptions: { input: resolve('marketing/site/index.html') },
  },
});
