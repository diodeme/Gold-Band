import { resolve } from 'node:path';
import { defineConfig, normalizePath, loadEnv } from 'vite';
import sirv from 'sirv';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { identitySensitiveDependencies } from '../../web/config/identity-sensitive-dependencies';
import { checkDataset, copyDataset } from './build-dataset.mjs';

const env = { ...loadEnv(process.env.NODE_ENV === 'production' ? 'production' : 'development', process.cwd(), ''), ...process.env };
const datasetSource = env.DEMO_DATASET_PATH ?? resolve('marketing/demo/data/ji-history');

export default defineConfig({
  base: process.env.VITE_DEMO_BASE ?? '/',
  root: 'marketing/demo',
  publicDir: '../../web/public',
  plugins: [
    {
      name: 'demo-dev-history',
      configureServer(server) {
        const serve = sirv(datasetSource, { dev: true, etag: true, single: false });
        server.middlewares.use('/data/ji-history', (request, response) => {
          if (request.url?.split('?')[0] === '/publish-index.json') { response.statusCode = 404; response.end(); return; }
          serve(request, response, () => { response.statusCode = 404; response.end(); });
        });
      },
    },
    {
      name: 'demo-history-dataset',
      apply: 'build',
      buildStart() {
        checkDataset(datasetSource, env.DEMO_ALLOW_INCOMPLETE_DATASET === '1');
      },
      writeBundle(options) {
        const source = datasetSource;
        copyDataset(source, resolve(options.dir!, 'data/ji-history'));
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
