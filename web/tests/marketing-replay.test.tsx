/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import Replay from '../../marketing/site/Replay';
import * as timeline from '../../marketing/site/timeline';

const harness = vi.hoisted(() => ({ players: [] as Array<{ time: number; destroy: ReturnType<typeof vi.fn>; play: ReturnType<typeof vi.fn>; pause: ReturnType<typeof vi.fn> }> }));
vi.mock('rrweb', () => ({ Replayer: class {
  time = 0;
  destroy = vi.fn(); play = vi.fn(); pause = vi.fn((time?: number) => { if (time !== undefined) this.time = time; }); on = vi.fn(); setConfig = vi.fn();
  getCurrentTime() { return this.time; }
  constructor(_events: unknown, options: { root: HTMLElement }) {
    const plane = document.createElement('div'); plane.className = 'replayer-wrapper'; options.root.append(plane);
    this.destroy.mockImplementation(() => plane.remove()); harness.players.push(this);
  }
} }));
vi.mock('@/components/ui/tooltip', () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  TooltipTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  TooltipContent: () => null,
}));
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const recording = { version: 1, reason: 'manual', bytes: 100, events: [{ type: 2, timestamp: 100, data: {} }, { type: 3, timestamp: 1100, data: {} }] };
const response = () => new Response(JSON.stringify(recording));
let container: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
let visible: (entries: unknown[]) => void;
let disconnect: ReturnType<typeof vi.fn>;
beforeEach(() => {
  harness.players.length = 0;
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
  disconnect = vi.fn();
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect = disconnect; });
  vi.stubGlobal('IntersectionObserver', class { constructor(callback: typeof visible) { visible = callback; } observe() {} disconnect = disconnect; });
  vi.stubGlobal('requestAnimationFrame', vi.fn(() => 1));
  vi.stubGlobal('cancelAnimationFrame', vi.fn());
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); });

describe('replay DOM lifecycle', () => {
  it.each(['chapter', 'language', 'theme', 'navigation'] as const)('releases every active resource on %s replacement', async change => {
    const removeMotion = vi.fn();
    vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: vi.fn(), removeEventListener: removeMotion }));
    const removeDocument = vi.spyOn(document, 'removeEventListener');
    const signals: AbortSignal[] = [];
    vi.stubGlobal('fetch', vi.fn(async (_url, input) => { signals.push(input.signal); return response(); }));
    try {
      await act(async () => root.render(<Replay language="en" chapter="before" autoPlay theme="dark" />));
      await act(async () => visible([{ isIntersecting: true }]));
      const oldPlayer = harness.players[0];
      const cancelled = vi.mocked(cancelAnimationFrame).mock.calls.length;
      await act(async () => root.render(change === 'navigation' ? null : <Replay
        language={change === 'language' ? 'zh' : 'en'} chapter={change === 'chapter' ? 'during' : 'before'}
        autoPlay theme={change === 'theme' ? 'light' : 'dark'} />));
      expect(signals[0].aborted).toBe(true);
      expect(oldPlayer.destroy).toHaveBeenCalledOnce();
      expect(disconnect.mock.calls.length).toBeGreaterThanOrEqual(2);
      expect(removeMotion).toHaveBeenCalledWith('change', expect.any(Function));
      expect(removeDocument).toHaveBeenCalledWith('visibilitychange', expect.any(Function));
      expect(vi.mocked(cancelAnimationFrame).mock.calls.length).toBeGreaterThan(cancelled);
      expect(container.querySelectorAll('.replayer-wrapper')).toHaveLength(change === 'navigation' ? 0 : 1);
      const plays = oldPlayer.play.mock.calls.length;
      await act(async () => document.dispatchEvent(new Event('visibilitychange')));
      expect(oldPlayer.play).toHaveBeenCalledTimes(plays);
    } finally { removeDocument.mockRestore(); }
  });
  it('rejects an unsupported asset before renderer allocation and recovers through retry', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({ ...recording, version: 2 }))).mockImplementation(async () => response()));
    await act(async () => root.render(<Replay language="en" chapter="before" autoPlay />));
    expect(harness.players).toHaveLength(0);
    expect(container.querySelector('[data-player-state="error"]')).not.toBeNull();
    expect(container.querySelector('.checkpoint-poster')).not.toBeNull();
    await act(async () => container.querySelector('button')!.click());
    expect(harness.players).toHaveLength(1);
    expect(container.querySelector('[data-player-state="ready"]')).not.toBeNull();
  });
  it('does not poll the native media query while painting camera frames', async () => {
    const matches = vi.fn(() => false);
    vi.stubGlobal('matchMedia', () => ({ get matches() { return matches(); }, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
    vi.stubGlobal('fetch', vi.fn(async () => response()));
    await act(async () => root.render(<Replay language="en" chapter="during" autoPlay />));
    const reads = matches.mock.calls.length;
    await act(async () => visible([{ isIntersecting: true }]));
    const paint = vi.mocked(requestAnimationFrame).mock.calls.at(-1)![0];
    await act(async () => paint(100));
    expect(matches).toHaveBeenCalledTimes(reads);
  });
  it('pauses while hidden or offscreen and resumes the same clock only when both gates allow it', async () => {
    const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(false);
    vi.stubGlobal('fetch', vi.fn(async () => response()));
    try {
      await act(async () => root.render(<Replay language="en" chapter="during" autoPlay />));
      const player = harness.players[0];
      player.time = 450;
      await act(async () => visible([{ isIntersecting: true }]));
      expect(player.play).toHaveBeenLastCalledWith(450);
      await act(async () => { hidden.mockReturnValue(true); document.dispatchEvent(new Event('visibilitychange')); });
      expect(player.pause).toHaveBeenLastCalledWith();
      await act(async () => visible([{ isIntersecting: false }]));
      const plays = player.play.mock.calls.length;
      await act(async () => { hidden.mockReturnValue(false); document.dispatchEvent(new Event('visibilitychange')); });
      expect(player.play).toHaveBeenCalledTimes(plays);
      await act(async () => visible([{ isIntersecting: true }]));
      expect(player.play).toHaveBeenLastCalledWith(450);
      await act(async () => container.querySelector('button')!.click());
      await act(async () => { visible([{ isIntersecting: false }]); visible([{ isIntersecting: true }]); });
      expect(player.play).toHaveBeenCalledTimes(plays + 1);
      expect(container.querySelector('button')?.getAttribute('aria-label')).toBe('Resume demo');
    } finally { hidden.mockRestore(); }
  });
  it('respects a runtime reduced-motion change and removes its listener when replaced', async () => {
    let changed: (event: { matches: boolean }) => void = () => {};
    const remove = vi.fn();
    const query = { matches: false, addEventListener: vi.fn((_event, listener) => { changed = listener; }), removeEventListener: remove };
    vi.stubGlobal('matchMedia', () => query);
    vi.stubGlobal('fetch', vi.fn(async () => response()));
    await act(async () => root.render(<Replay language="en" chapter="during" autoPlay />));
    await act(async () => visible([{ isIntersecting: true }]));
    const player = harness.players[0];
    await act(async () => { changed({ matches: true }); });
    expect(player.pause).toHaveBeenLastCalledWith();
    expect(container.querySelector('button')?.getAttribute('aria-label')).toBe('Resume demo');
    const listener = changed;
    await act(async () => root.render(null));
    expect(remove).toHaveBeenCalledWith('change', listener);
  });
  it('shows the target-theme semantic checkpoint while loading and retries without returning to the opening', async () => {
    const frame = { x: 0, y: 0, width: 1, height: 1 };
    const asset = { ...recording, events: [recording.events[0], { type: 5, timestamp: 300, data: { tag: 'site-shot', payload: { id: 'report-result', desktop: frame, mobile: frame, speed: 1, transition: 0 } } }, recording.events[1]] };
    const pending: Array<{ resolve: (response: Response) => void }> = [];
    vi.stubGlobal('fetch', vi.fn(() => new Promise<Response>(resolve => pending.push({ resolve }))));
    await act(async () => root.render(<Replay language="en" chapter="after" autoPlay theme="dark" />));
    await act(async () => pending[0].resolve(new Response(JSON.stringify(asset))));
    harness.players[0].time = 600;
    await act(async () => container.querySelector('button')!.click());
    await act(async () => root.render(<Replay language="en" chapter="after" autoPlay theme="light" />));
    const poster = () => container.querySelector('[data-checkpoint-poster="report-result"]');
    expect(poster()?.getAttribute('data-poster-theme')).toBe('light');
    expect(poster()?.querySelector('img')?.getAttribute('src')).toContain('en-after-light-report-result');
    expect(container.querySelectorAll('.replayer-wrapper')).toHaveLength(0);
    await act(async () => pending[1].resolve(new Response('', { status: 503 })));
    expect(container.querySelector('[data-player-state="error"]')).not.toBeNull();
    expect(poster()).not.toBeNull();
    await act(async () => container.querySelector('button')!.click());
    expect(poster()).not.toBeNull();
    await act(async () => pending[2].resolve(new Response(JSON.stringify(asset))));
    expect(harness.players.at(-1)?.pause).toHaveBeenCalledWith(600);
    expect(container.querySelector('.checkpoint-poster')).toBeNull();
    expect(container.querySelector('button')?.getAttribute('aria-label')).toBe('Resume demo');
  });
  it('keeps the last checkpoint when an unfinished theme load is replaced', async () => {
    const pending: Array<{ signal: AbortSignal; resolve: (response: Response) => void }> = [];
    vi.stubGlobal('fetch', vi.fn((_url, input) => new Promise<Response>(resolve => pending.push({ signal: input.signal, resolve }))));
    await act(async () => root.render(<Replay language="en" chapter="after" autoPlay theme="dark" />));
    await act(async () => pending[0].resolve(response()));
    harness.players[0].time = 500;
    await act(async () => container.querySelector('button')!.click());
    await act(async () => root.render(<Replay language="en" chapter="after" autoPlay theme="light" />));
    await act(async () => root.render(<Replay language="en" chapter="after" autoPlay theme="dark" />));
    expect(pending[1].signal.aborted).toBe(true);
    await act(async () => pending[2].resolve(response()));
    expect(harness.players.at(-1)?.pause).toHaveBeenCalledWith(500);
    await act(async () => pending[1].resolve(response()));
    expect(harness.players).toHaveLength(2);
    expect(container.querySelector('button')?.getAttribute('aria-label')).toBe('Resume demo');
  });
  it('restores the semantic step and paused intent across theme assets with different timings', async () => {
    const frame = { x: 0, y: 0, width: 1, height: 1 };
    const asset = (at: number, end: number) => ({ ...recording, events: [recording.events[0],
      { type: 5, timestamp: at + 100, data: { tag: 'site-shot', payload: { id: 'review', desktop: frame, mobile: frame, speed: 1, transition: 0 } } },
      { type: 3, timestamp: end + 100, data: {} }] });
    const fetcher = vi.fn(async (url: string) => new Response(JSON.stringify(url.includes('-light') ? asset(400, 2000) : asset(200, 1000))));
    vi.stubGlobal('fetch', fetcher);
    await act(async () => root.render(<Replay language="en" chapter="after" autoPlay theme="dark" />));
    harness.players[0].time = 600;
    await act(async () => container.querySelector('button')!.click());
    await act(async () => root.render(<Replay language="en" chapter="after" autoPlay theme="light" />));
    expect(harness.players).toHaveLength(2);
    expect(harness.players[0].destroy).toHaveBeenCalledOnce();
    expect(harness.players[1].pause).toHaveBeenCalledWith(1200);
    expect(container.querySelector('button')?.getAttribute('aria-label')).toBe('Resume demo');
  });
  it('uses the layout mode rather than the stage width to choose its camera track', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => response()));
    const sample = vi.spyOn(timeline, 'sampleTrack');
    try {
      await act(async () => root.render(<Replay language="en" chapter="during" autoPlay mobile={false} />));
      expect(sample.mock.calls.at(-1)?.[2]).toBe(false);
      await act(async () => root.render(<Replay language="en" chapter="during" autoPlay mobile />));
      expect(sample.mock.calls.at(-1)?.[2]).toBe(true);
    } finally { sample.mockRestore(); }
  });
  it('keeps the visible pause control aligned when autoplay changes', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => response()));
    await act(async () => root.render(<Replay language="en" chapter="during" autoPlay />));
    expect(container.querySelector('button')?.getAttribute('aria-label')).toBe('Pause demo');
    await act(async () => root.render(<Replay language="en" chapter="during" autoPlay={false} />));
    expect(container.querySelector('button')?.getAttribute('aria-label')).toBe('Resume demo');
    await act(async () => visible([{ isIntersecting: true }]));
    expect(harness.players.at(-1)?.play).not.toHaveBeenCalled();
    await act(async () => container.querySelector('button')!.click());
    expect(harness.players.at(-1)?.play).toHaveBeenCalledOnce();
  });
  it('aborts replaced requests and ignores their late success', async () => {
    const pending: Array<{ signal: AbortSignal; resolve: (response: Response) => void }> = [];
    vi.stubGlobal('fetch', vi.fn((_url, input) => new Promise<Response>(resolve => pending.push({ signal: input.signal, resolve }))));
    await act(async () => root.render(<Replay language="en" chapter="before" autoPlay />));
    await act(async () => root.render(<Replay language="en" chapter="during" autoPlay />));
    expect(pending[0].signal.aborted).toBe(true);
    await act(async () => pending[1].resolve(response()));
    expect(harness.players).toHaveLength(1);
    await act(async () => pending[0].resolve(response()));
    expect(harness.players).toHaveLength(1);
    expect(container.querySelector('[data-player-state]')?.getAttribute('data-player-state')).toBe('ready');
  });
  it('destroys the renderer, observers and request on unmount', async () => {
    let signal: AbortSignal;
    vi.stubGlobal('fetch', vi.fn(async (_url, input) => { signal = input.signal; return response(); }));
    await act(async () => root.render(<Replay language="en" chapter="during" autoPlay />));
    await act(async () => root.render(null));
    expect(signal!.aborted).toBe(true);
    expect(harness.players[0].destroy).toHaveBeenCalledOnce();
    expect(disconnect).toHaveBeenCalledTimes(3);
    expect(container.querySelector('.replayer-wrapper')).toBeNull();
    expect(cancelAnimationFrame).toHaveBeenCalled();
  });
});
