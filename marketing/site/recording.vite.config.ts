import { resolve } from 'node:path';
import { mergeConfig } from 'vite';
import { mkdir, writeFile, rename } from 'node:fs/promises';
import { z } from 'zod';
import { portableRecording } from '../../scripts/site-recording-assets.mjs';
import { readRecording } from './timeline';
import { randomUUID } from 'node:crypto';
import { createReviewRepository } from '../../scripts/site-review-repository.mjs';
import site from './vite.config';
const repositories = new Map<string, ReturnType<typeof createReviewRepository>>();

export default mergeConfig(site, {
  server: { hmr: false, watch: { ignored: ['**/media/**'] } },
  plugins: [{
    name: 'site-capture-output',
    async closeBundle() {
      const closing = [...repositories.values()];
      repositories.clear();
      await Promise.allSettled(closing.map(async repository => (await repository).dispose()));
    },
    configureServer(server) {
      const reviewPath = z.enum(['src/config.json', 'docs/workspace-notes.md']);
      const reviewCommand = z.discriminatedUnion('action', [
        z.object({ action: z.literal('status') }),
        z.object({ action: z.literal('read'), path: reviewPath }),
        z.object({ action: z.literal('write'), path: reviewPath, expectedHash: z.string().length(64), content: z.string().max(16384) }),
        z.object({ action: z.literal('comparison'), path: reviewPath, area: z.enum(['staged', 'unstaged']) }),
        z.object({ action: z.literal('stage'), paths: z.array(reviewPath).min(1).max(2), expectedRevision: z.string().length(64).nullish() }),
        z.object({ action: z.literal('commit'), subject: z.string().trim().min(1).max(120), expectedRevision: z.string().length(64).nullish() }),
      ]);
      server.middlewares.use('/__site-review', async (request, response) => {
        response.setHeader('Content-Type', 'application/json');
        try {
          if (request.method !== 'POST' || request.headers.origin !== `http://${request.headers.host}`) throw { code: 'site.review-origin-rejected' };
          const chunks: Buffer[] = [];
          let bytes = 0;
          for await (const chunk of request) { bytes += chunk.length; if (bytes > 24 * 1024) throw { code: 'site.review-budget' }; chunks.push(chunk); }
          const input = z.discriminatedUnion('action', [
            z.object({ action: z.literal('initialize'), language: z.enum(['zh', 'en']) }),
            z.object({ action: z.literal('close'), session: z.string().uuid() }),
            z.object({ action: z.literal('execute'), session: z.string().uuid(), command: reviewCommand }),
          ]).parse(JSON.parse(Buffer.concat(chunks).toString('utf8')));
          if (input.action === 'initialize') {
            const previous = [...repositories.values()];
            repositories.clear();
            const session = randomUUID();
            const repository = (async () => {
              await Promise.allSettled(previous.map(async value => (await value).dispose()));
              return createReviewRepository(input.language);
            })();
            repositories.set(session, repository);
            try { await repository; } catch (error) { repositories.delete(session); throw error; }
            response.end(JSON.stringify({ session }));
          } else {
            const repository = await repositories.get(input.session);
            if (!repository) throw { code: 'site.review-session-missing' };
            if (input.action === 'close') { repositories.delete(input.session); await repository.dispose(); response.end('{}'); }
            else response.end(JSON.stringify(await repository.run(input.command)));
          }
        } catch (error) {
          response.statusCode = 400;
          const code = typeof error === 'object' && error !== null && 'code' in error && typeof error.code === 'string' ? error.code : 'site.review-operation-failed';
          response.end(JSON.stringify({ code, params: {} }));
        }
      });
      server.middlewares.use((request, _response, next) => {
        if (request.headers.accept?.includes('text/html') && request.url?.startsWith('/chat')) {
          request.url = `/preview.html${request.url.includes('?') ? request.url.slice(request.url.indexOf('?')) : ''}`;
        }
        next();
      });
      server.middlewares.use('/__site-recording', async (request, response) => {
        try {
          const origin = `http://${request.headers.host}`;
          if (request.method !== 'POST' || request.headers.origin !== origin) { response.statusCode = 403; response.end(); return; }
          let bytes = 0;
          const chunks: Buffer[] = [];
          for await (const chunk of request) {
            bytes += chunk.length;
            if (bytes > 13 * 1024 * 1024) throw new Error('recording-budget');
            chunks.push(chunk);
          }
          const input = z.object({ language: z.enum(['zh', 'en']), scene: z.enum(['before', 'during', 'after', 'personalize']), theme: z.enum(['light', 'dark']), recording: z.unknown().optional(), poster: z.string().max(4 * 1024 * 1024).optional() }).parse(JSON.parse(Buffer.concat(chunks).toString('utf8')));
          const name = `${input.language}-${input.scene}${input.theme === 'light' ? '-light' : ''}`;
          const directory = resolve('marketing/site/media');
          await mkdir(directory, { recursive: true });
          if (input.recording) {
            const portable = portableRecording(input.recording, origin);
            const validated = readRecording(portable);
            const path = resolve(directory, `${name}.json`);
            await writeFile(`${path}.tmp`, JSON.stringify(portable));
            await rename(`${path}.tmp`, path);
            response.setHeader('Content-Type', 'application/json');
            response.end(JSON.stringify({ name, bytes: portable.bytes, events: portable.events.length, duration: validated.duration, checkpoints: validated.shots.map(shot => shot.id) }));
          } else if (input.poster) {
            const png = Buffer.from(input.poster, 'base64');
            if (!png.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))) throw new Error('poster-invalid');
            await writeFile(resolve(directory, `${name}.png`), png);
            response.end(JSON.stringify({ name }));
          } else throw new Error('capture-empty');
        } catch {
          response.statusCode = 400;
          response.end(JSON.stringify({ code: 'site.capture-invalid' }));
        }
      });
    },
  }],
  build: {
    outDir: '../../.codex-temp/site-recording-dist',
    rollupOptions: { input: resolve('marketing/site/preview.html') },
  },
});
