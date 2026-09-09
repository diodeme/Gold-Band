import { resolve } from 'node:path';
import { defineConfig, normalizePath } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { identitySensitiveDependencies } from '../../web/config/identity-sensitive-dependencies';

export default defineConfig({
  root: 'marketing/recording',
  publicDir: '../../web/public',
  plugins: [{
    name: 'recording-runtime', enforce: 'pre',
    resolveId(source, importer) {
      if (importer?.replaceAll('\\', '/').endsWith('/web/src/api/client.ts') && (source === './browser' || source === './desktop')) {
        return normalizePath(resolve('marketing/recording/runtime.ts'));
      }
    },
  }, react(), tailwindcss()],
  resolve: { alias: { '@': resolve('web/src') }, dedupe: [...identitySensitiveDependencies] },
  server: { strictPort: true },
  build: { target: 'safari15.4', outDir: '../../.codex-temp/recording-dist', emptyOutDir: true },
});
