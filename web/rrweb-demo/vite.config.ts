import { resolve } from 'node:path';
import { mergeConfig } from 'vite';
import desktop from '../vite.config';

export default mergeConfig(desktop, {
  build: {
    outDir: '../.codex-temp/rrweb-demo-dist',
    rollupOptions: {
      input: {
        demo: resolve('web/rrweb-demo/index.html'),
        capture: resolve('web/rrweb-demo/capture.html'),
      },
    },
  },
});
