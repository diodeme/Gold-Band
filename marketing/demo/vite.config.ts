import { resolve } from 'node:path';
import { defineConfig, normalizePath } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { identitySensitiveDependencies } from '../../web/config/identity-sensitive-dependencies';

const base = process.env.WEBSITE_BASE ?? '/';
const outDir = process.env.WEBSITE_DEMO_DIR ? resolve(process.env.WEBSITE_DEMO_DIR) : '../../.codex-temp/demo-dist';

export default defineConfig({
  root: 'marketing/demo',
  base,
  publicDir: '../../web/public',
  plugins: [
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
  build: { target: 'safari15.4', outDir, emptyOutDir: true },
});
