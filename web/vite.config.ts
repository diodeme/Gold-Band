import { fileURLToPath, URL } from 'node:url';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { identitySensitiveDependencies } from './config/identity-sensitive-dependencies';
import { releaseNotesPlugin } from './config/release-notes-plugin';

export default defineConfig({
  root: 'web',
  plugins: [react(), tailwindcss(), releaseNotesPlugin(fileURLToPath(new URL('..', import.meta.url)))],
  resolve: {
    dedupe: [...identitySensitiveDependencies],
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    target: 'safari15.4',
  },
});
