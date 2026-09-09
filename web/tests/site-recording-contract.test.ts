import { describe, expect, it } from 'vitest';
import { browserApi as sharedApi } from '../src/api/browser';
import { createRecordingApi } from '../../marketing/recording/runtime';
import { SceneAssetSchema, SceneManifestSchema, playbackPosition, playbackTime, selectSceneAsset } from '../../marketing/site/replay-model';

const frame = { timeMs: 0, rect: { x: 0, y: 0, width: 1, height: 1 }, zoom: 1, transitionMs: 450 };
const asset = () => SceneAssetSchema.parse({
  version: 1, scene: 'during', language: 'zh', theme: 'dark', rrwebVersion: '2.1.1',
  eventsUrl: '/media/v1/during/zh/dark/events.json', sha256: 'a'.repeat(64), byteLength: 100, eventCount: 2,
  durationMs: 1000, width: 1440, height: 880, poster: '/poster.webp',
  checkpoints: [{ stepId: 'output', startMs: 0, endMs: 600, poster: '/output.webp' }, { stepId: 'branch', startMs: 600, endMs: 1000, poster: '/branch.webp' }],
  camera: { desktop: [frame], mobile: [frame] }, pace: [{ startMs: 0, endMs: 1000, fromRate: 1, toRate: 1.5 }],
});

describe('recording runtime isolation', () => {
  it('isolates method overrides and preferences from both shared and other recording runtimes', async () => {
    const original = sharedApi.getConversationRun;
    const first = createRecordingApi();
    const second = createRecordingApi();
    const next = async () => { throw { code: 'recording.test' }; };
    first.getConversationRun = next;
    expect(first.getConversationRun).toBe(next);
    expect(sharedApi.getConversationRun).toBe(original);
    expect(second.getConversationRun).not.toBe(next);
    const preferences = (await first.getAppBootstrap()).preferences;
    const before = (await second.getAppBootstrap()).preferences;
    await first.saveDesktopPreferences({ ...preferences.appearance, colorScheme: 'light' }, preferences.personalization, 'en', preferences.useLocalClaude, preferences.verboseLogging);
    expect((await second.getAppBootstrap()).preferences).toEqual(before);
    await expect(first.openExternalUrl('https://example.com')).rejects.toMatchObject({ code: 'demo.operation-unavailable' });
  });
});

describe('semantic recording asset contract', () => {
  it('accepts a bounded phone take paired to the same business steps', () => {
    const mobile = { ...asset(), width: 320, height: 780 };
    expect(SceneManifestSchema.safeParse({ version: 1, assets: [asset()], mobileAssets: [mobile] }).success).toBe(true);
    mobile.checkpoints = mobile.checkpoints.map(point => ({ ...point, stepId: 'unpaired-' + point.stepId }));
    expect(SceneManifestSchema.safeParse({ version: 1, assets: [asset()], mobileAssets: [mobile] }).success).toBe(false);
  });
  it('selects one matching take and preserves semantic position across viewport formats', () => {
    const primary = asset();
    const phone = { ...asset(), width: 320, height: 780, eventsUrl: '/phone.json' };
    phone.checkpoints = phone.checkpoints.map((point, index) => ({ ...point, startMs: index ? 800 : 0, endMs: index ? 1000 : 800 }));
    const manifest = SceneManifestSchema.parse({ version: 1, assets: [primary], mobileAssets: [phone] });
    const selected = selectSceneAsset(manifest, 'during', 'zh', 'dark', true)!;
    expect(selected.eventsUrl).toBe('/phone.json');
    expect(playbackTime(selected, playbackPosition(primary, 300))).toBe(400);
    expect(selectSceneAsset(manifest, 'during', 'zh', 'dark', false)?.width).toBe(1440);
    expect(selectSceneAsset(manifest, 'during', 'en', 'dark', true)).toBeUndefined();
    expect(SceneManifestSchema.safeParse({ ...manifest, mobileAssets: [phone, phone] }).success).toBe(false);
  });
  it('maps a semantic position across recordings with different timings', () => {
    const source = asset();
    const target = asset();
    target.checkpoints[0].endMs = 800;
    target.checkpoints[1].startMs = 800;
    expect(playbackTime(target, playbackPosition(source, 300))).toBe(400);
    expect(playbackTime(target, playbackPosition(source, 800))).toBe(900);
    expect(playbackPosition(source, 600).stepId).toBe('branch');
    expect(() => playbackTime(target, { scene: 'during', stepId: 'missing', fraction: 0 })).toThrow();
  });
  it('rejects gaps, duplicate steps, out-of-bounds cameras and over-budget files', () => {
    const invalid = [
      { ...asset(), byteLength: 12 * 1024 * 1024 + 1 },
      { ...asset(), pace: [{ startMs: 10, endMs: 1000, fromRate: 1, toRate: 1 }] },
      { ...asset(), checkpoints: asset().checkpoints.map((point) => ({ ...point, stepId: 'same' })) },
      { ...asset(), camera: { desktop: [{ ...frame, rect: { x: 0.8, y: 0, width: 0.5, height: 1 } }], mobile: [frame] } },
      { ...asset(), eventsUrl: 'http://127.0.0.1:1442/events.json' },
    ];
    invalid.forEach((value) => expect(SceneAssetSchema.safeParse(value).success).toBe(false));
  });
  it('rejects duplicate variants and semantic mismatches without requiring unpublished scenes', () => {
    expect(SceneManifestSchema.safeParse({ version: 1, assets: [asset()] }).success).toBe(true);
    expect(SceneManifestSchema.safeParse({ version: 1, assets: [asset(), asset()] }).success).toBe(false);
    const other = asset(); other.theme = 'light'; other.checkpoints[0].stepId = 'different';
    expect(SceneManifestSchema.safeParse({ version: 1, assets: [asset(), other] }).success).toBe(false);
  });
});
