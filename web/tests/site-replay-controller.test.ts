import { describe, expect, it, vi } from 'vitest';
import { cameraAt, paceAt, ReplayController, type ReplayEngine } from '../../marketing/site/replay-controller';
import { SceneAssetSchema } from '../../marketing/site/replay-model';

function asset(theme: 'light' | 'dark' = 'dark') {
  const frame = { timeMs: 0, rect: { x: 0, y: 0, width: 1, height: 1 }, zoom: 1, transitionMs: 400 };
  return SceneAssetSchema.parse({ version: 1, scene: 'during', language: 'zh', theme, rrwebVersion: '2.1.1',
    eventsUrl: '/events.json', sha256: 'a'.repeat(64), byteLength: 100, eventCount: 2, durationMs: 1000,
    width: 1440, height: 900, poster: '/poster.png',
    checkpoints: [{ stepId: 'output', startMs: 0, endMs: 500, poster: '/output.png' }, { stepId: 'branch', startMs: 500, endMs: 1000, poster: '/branch.png' }],
    camera: { desktop: [frame, { ...frame, timeMs: 500, rect: { x: 0.5, y: 0, width: 0.5, height: 0.5 } }], mobile: [frame] },
    pace: [{ startMs: 0, endMs: 1000, fromRate: 1, toRate: 2 }],
  });
}
function harness(load = vi.fn(async () => ({}))) {
  let time = 0;
  let next = 0;
  const frames = new Map<number, () => void>();
  const engines: ReplayEngine[] = [];
  const dependencies = {
    load,
    create: vi.fn(() => { const engine = { getCurrentTime: () => time, pause: vi.fn((at?: number) => { if (at !== undefined) time = at; }), play: vi.fn((at: number) => { time = at; }), destroy: vi.fn(), setConfig: vi.fn() }; engines.push(engine); return engine; }),
    requestFrame: (callback: () => void) => { frames.set(++next, callback); return next; }, cancelFrame: (id: number) => { frames.delete(id); },
    paint: vi.fn(), changed: vi.fn(),
  };
  return { controller: new ReplayController(dependencies), dependencies, engines, frames, setTime: (at: number) => { time = at; }, tick: () => { const callbacks = [...frames.values()]; frames.clear(); callbacks.forEach(callback => callback()); } };
}
describe('independent replay lifetime', () => {
  it('reads the engine clock for camera and pace and freezes user pause across visibility changes', async () => {
    const h = harness(); await h.controller.select(asset());
    h.setTime(250); h.tick();
    expect(h.dependencies.paint).toHaveBeenLastCalledWith(asset(), 250, false);
    expect(h.engines[0].setConfig).toHaveBeenLastCalledWith({ speed: 1.25 });
    h.controller.setPlaying(false); h.controller.setEnvironment({ hidden: true }); h.controller.setEnvironment({ hidden: false });
    expect(h.frames.size).toBe(0);
    h.controller.setPlaying(true); expect(h.frames.size).toBe(1);
    h.controller.setEnvironment({ visible: false }); expect(h.frames.size).toBe(0);
    h.controller.setEnvironment({ visible: true }); expect(h.frames.size).toBe(1);
    h.controller.dispose(); expect(h.frames.size).toBe(0); expect(h.engines[0].destroy).toHaveBeenCalledTimes(1);
  });
  it('starts reduced motion paused and never animates its camera after explicit play', async () => {
    const h = harness(); h.controller.setEnvironment({ reduced: true }); await h.controller.select(asset());
    expect(h.engines[0].play).not.toHaveBeenCalled();
    h.controller.setPlaying(true); h.setTime(200); h.tick();
    expect(h.dependencies.paint).toHaveBeenLastCalledWith(asset(), 200, true); h.controller.dispose();
  });
  it('preserves semantic position across timing differences and releases the previous instance first', async () => {
    const h = harness(); await h.controller.select(asset()); h.setTime(250); h.controller.setPlaying(false);
    const target = asset('light'); target.checkpoints[0].endMs = 800; target.checkpoints[1].startMs = 800;
    await h.controller.select(target);
    expect(h.engines[0].destroy).toHaveBeenCalledTimes(1);
    expect(h.engines[1].pause).toHaveBeenCalledWith(400);
    expect(h.engines[1].play).not.toHaveBeenCalled(); h.controller.dispose();
  });
  it('rejects late success and failure even when a loader ignores abort', async () => {
    const pending: { resolve: (value: unknown) => void; reject: (error: unknown) => void }[] = [];
    const load = vi.fn(() => new Promise((resolve, reject) => pending.push({ resolve, reject })));
    const h = harness(load); const first = h.controller.select(asset()); const second = h.controller.select(asset('light'));
    pending[1].resolve({}); await second; pending[0].reject(new Error('old')); await first;
    expect(h.dependencies.create).toHaveBeenCalledTimes(1);
    const third = h.controller.select(asset()); h.controller.dispose(); pending[2].resolve({}); await third;
    expect(h.dependencies.create).toHaveBeenCalledTimes(1);
    expect(h.dependencies.changed).toHaveBeenLastCalledWith(expect.objectContaining({ status: 'disposed' }));
  });
  it('replays from the start after reaching the end without accumulating RAFs', async () => {
    const h = harness(); await h.controller.select(asset()); h.setTime(1000); h.tick();
    expect(h.frames.size).toBe(0); h.controller.setPlaying(true);
    expect(h.engines[0].play).toHaveBeenLastCalledWith(0); expect(h.frames.size).toBe(1); h.controller.dispose();
  });
  it('cancels pending event loads while a replacement manifest is being loaded', async () => {
    let resolve!: (value: unknown) => void;
    const h = harness(vi.fn(() => new Promise(done => { resolve = done; })));
    const selected = h.controller.select(asset());
    h.controller.suspend(); resolve({}); await selected;
    expect(h.dependencies.create).not.toHaveBeenCalled();
    expect(h.frames.size).toBe(0); h.controller.dispose();
  });
});
describe('camera and pace tracks', () => {
  it('interpolates smoothly from the previous camera using raw time and clamps stage edges', () => {
    const source = asset(); const start = cameraAt(source, 500, 720, 450, false, false);
    const middle = cameraAt(source, 700, 720, 450, false, false);
    const end = cameraAt(source, 900, 720, 450, false, false);
    expect(start.scale).toBe(0.5); expect(end.scale).toBe(1); expect(middle.scale).toBe(0.75);
    expect(end.x).toBe(-720); expect(paceAt(source, 500)).toBe(1.5);
    expect(cameraAt(source, 500, 720, 450, false, true)).toEqual(end);
  });
});
