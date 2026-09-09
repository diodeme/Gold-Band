import { afterEach, describe, expect, it, vi } from 'vitest';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { resolve, dirname, sep } from 'node:path';
import { loadSceneEvents } from '../../marketing/site/replay-assets';
import { SceneManifestSchema, type SceneAsset } from '../../marketing/site/replay-model';

function fixture() {
  const recording = { version: 1, reason: 'manual', bytes: 0, events: [{ type: 4, timestamp: 1000, data: { width: 1440, height: 900, href: '/' } }, { type: 5, timestamp: 2000, data: { tag: 'recording-end', payload: {} } }] };
  while (recording.bytes !== Buffer.byteLength(JSON.stringify(recording))) recording.bytes = Buffer.byteLength(JSON.stringify(recording));
  const text = JSON.stringify(recording);
  const frame = { timeMs: 0, rect: { x: 0, y: 0, width: 1, height: 1 }, zoom: 1, transitionMs: 0 };
  const asset: SceneAsset = { version: 1, scene: 'during', language: 'zh', theme: 'dark', rrwebVersion: '2.1.1', eventsUrl: '/events.json',
    sha256: createHash('sha256').update(text).digest('hex'), byteLength: Buffer.byteLength(text), eventCount: 2, durationMs: 1000,
    width: 1440, height: 900, poster: '/poster.png', checkpoints: [{ stepId: 'output', startMs: 0, endMs: 1000, poster: '/poster.png' }],
    camera: { desktop: [frame], mobile: [frame] }, pace: [{ startMs: 0, endMs: 1000, fromRate: 1, toRate: 1 }] };
  return { asset, text };
}
afterEach(() => vi.unstubAllGlobals());
describe('published scene event loading', () => {
  it.each([['workflow', 'during'], ['before', 'before'], ['after', 'after']])('ships %s variants and dependencies at the normal build input', async (folder, scene) => {
    const directory = resolve(`marketing/site/media/${folder}`);
    const manifest = SceneManifestSchema.parse(JSON.parse(await readFile(resolve(directory, 'manifest.json'), 'utf8')));
    const localFile = (url: string) => {
      expect(url.startsWith(`/media/${folder}/`)).toBe(true);
      const path = resolve(directory, url.slice(`/media/${folder}/`.length));
      expect(path.startsWith(directory + sep)).toBe(true);
      return path;
    };
    for (const variants of [manifest.assets, manifest.mobileAssets!]) {
      expect(variants.filter(asset => asset.scene === scene).map(asset => `${asset.language}/${asset.theme}`).sort())
        .toEqual(['en/dark', 'en/light', 'zh/dark', 'zh/light']);
      for (const asset of variants.filter(asset => asset.scene === scene)) {
        const bytes = await readFile(localFile(asset.eventsUrl));
        vi.stubGlobal('fetch', vi.fn(async () => new Response(new Uint8Array(bytes))));
        expect(await loadSceneEvents(asset, new AbortController().signal)).toHaveLength(asset.eventCount);
        for (const poster of new Set([asset.poster, ...asset.checkpoints.map(point => point.poster)])) {
          expect((await readFile(localFile(poster))).subarray(0, 8).toString('hex')).toBe('89504e470d0a1a0a');
        }
        const resources = JSON.parse(await readFile(resolve(dirname(localFile(asset.eventsUrl)), 'resources.json'), 'utf8'));
        expect(resources.version).toBe(1);
        for (const resource of resources.resources) {
          const content = await readFile(localFile(resource.url));
          expect(content.byteLength).toBe(resource.byteLength);
          expect(createHash('sha256').update(content).digest('hex')).toBe(resource.sha256);
        }
      }
    }
  });
  it('loads the exact UTF-8 file and checks manifest count and duration', async () => {
    const { asset, text } = fixture();
    vi.stubGlobal('fetch', vi.fn(async () => new Response(text)));
    expect(await loadSceneEvents(asset, new AbortController().signal)).toHaveLength(2);
    await expect(loadSceneEvents({ ...asset, eventCount: 3 }, new AbortController().signal)).rejects.toMatchObject({ code: 'site.asset-invalid' });
  });
  it('rejects stale or modified bytes before parsing them', async () => {
    const { asset, text } = fixture();
    vi.stubGlobal('fetch', vi.fn(async () => new Response(text.replace('manual', 'broken'))));
    await expect(loadSceneEvents(asset, new AbortController().signal)).rejects.toMatchObject({ code: 'site.asset-hash' });
    vi.stubGlobal('fetch', vi.fn(async () => new Response(text + ' ')));
    await expect(loadSceneEvents(asset, new AbortController().signal)).rejects.toMatchObject({ code: 'site.asset-size' });
  });
});
