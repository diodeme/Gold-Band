// @vitest-environment jsdom

import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { DesktopWindowFrame } from '../src/components/DesktopWindowFrame';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const host = vi.hoisted(() => ({
  innerSize: vi.fn(),
  onResized: vi.fn(),
  onScaleChanged: vi.fn(),
  native: true,
}));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => host }));
vi.mock('../src/api/shared', () => ({ isTauriRuntime: () => host.native }));

type Size = { width: number; height: number };
let resized: (event: { payload: Size }) => void;
let scaled: (event: { payload: { size: Size; scaleFactor: number } }) => void;
let root: Root;
let container: HTMLDivElement;
let disposeResize: ReturnType<typeof vi.fn>;
let disposeScale: ReturnType<typeof vi.fn>;

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal('devicePixelRatio', 1.75);
  host.native = true;
  host.innerSize.mockResolvedValue({ width: 1795, height: 1260 });
  disposeResize = vi.fn();
  disposeScale = vi.fn();
  host.onResized.mockImplementation(async (callback: typeof resized) => { resized = callback; return disposeResize; });
  host.onScaleChanged.mockImplementation(async (callback: typeof scaled) => { scaled = callback; return disposeScale; });
  container = document.createElement('div');
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

async function mount(frameStyle: 'app-outline' | 'native-compositor' = 'app-outline', child = <span>content</span>) {
  await act(async () => root.render(<DesktopWindowFrame frameStyle={frameStyle}>{child}</DesktopWindowFrame>));
  await act(async () => vi.advanceTimersByTime(20));
  return container.firstElementChild as HTMLDivElement;
}

function expectSize(frame: HTMLDivElement, width: number, height: number) {
  expect(parseFloat(frame.style.width) * devicePixelRatio).toBeCloseTo(width, 5);
  expect(parseFloat(frame.style.height) * devicePixelRatio).toBeCloseTo(height, 5);
}

it('fits the native client instead of the oversized WebView viewport, including after right-edge resize', async () => {
  vi.stubGlobal('innerWidth', 1026);
  vi.stubGlobal('innerHeight', 721);
  const renderContent = vi.fn(() => <span>content</span>);
  const Content = renderContent;
  const frame = await mount('app-outline', <Content />);
  expectSize(frame, 1795, 1260);
  await act(async () => {
    resized({ payload: { width: 1793, height: 1260 } });
    resized({ payload: { width: 1794, height: 1260 } });
    resized({ payload: { width: 1800, height: 1260 } });
  });
  expectSize(frame, 1795, 1260);
  await act(async () => vi.advanceTimersByTime(20));
  expectSize(frame, 1800, 1260);
  expect(renderContent).toHaveBeenCalledOnce();
  expect(host.innerSize).toHaveBeenCalledOnce();
});

it('rejects a late startup size after a native resize', async () => {
  let resolveSize!: (size: Size) => void;
  host.innerSize.mockReturnValue(new Promise<Size>((resolve) => { resolveSize = resolve; }));
  const frame = await mount();
  await act(async () => {
    resized({ payload: { width: 1800, height: 1260 } });
    resolveSize({ width: 1795, height: 1260 });
    await vi.advanceTimersByTimeAsync(20);
  });
  expectSize(frame, 1800, 1260);
});

it('reprojects physical dimensions on scale changes and ignores minimized zero sizes', async () => {
  const frame = await mount();
  vi.stubGlobal('devicePixelRatio', 1.25);
  await act(async () => {
    scaled({ payload: { size: { width: 1500, height: 1000 }, scaleFactor: 1.25 } });
    await vi.advanceTimersByTimeAsync(20);
  });
  expectSize(frame, 1500, 1000);
  await act(async () => {
    resized({ payload: { width: 0, height: 0 } });
    await vi.advanceTimersByTimeAsync(20);
  });
  expectSize(frame, 1500, 1000);
});

it('disposes native subscriptions and pending animation frames on policy changes', async () => {
  const frame = await mount();
  await act(async () => resized({ payload: { width: 1900, height: 1300 } }));
  await mount('native-compositor');
  expect(disposeResize).toHaveBeenCalledOnce();
  expect(disposeScale).toHaveBeenCalledOnce();
  expect(frame.style.width).toBe('');
  expect(frame.style.height).toBe('');
  expect(vi.getTimerCount()).toBe(0);
});

it('leaves ordinary browser and native-compositor sizing to CSS', async () => {
  host.native = false;
  const frame = await mount();
  expect(frame.style.width).toBe('');
  expect(host.onResized).not.toHaveBeenCalled();
  host.native = true;
  await mount('native-compositor');
  expect(host.onResized).not.toHaveBeenCalled();
});

it('reprojects after WebView DPR changes without querying native size again', async () => {
  const frame = await mount();
  for (const scale of [1, 1.25, 1.5, 1.75, 2]) {
    vi.stubGlobal('devicePixelRatio', scale);
    await act(async () => {
      window.dispatchEvent(new Event('resize'));
      await vi.advanceTimersByTimeAsync(20);
    });
    expectSize(frame, 1795, 1260);
  }
  expect(host.innerSize).toHaveBeenCalledOnce();
  await act(async () => root.unmount());
  window.dispatchEvent(new Event('resize'));
  expect(vi.getTimerCount()).toBe(0);
});

it('disposes subscriptions that complete after unmount without reading a startup size', async () => {
  let resolveSubscription!: (dispose: () => void) => void;
  host.onResized.mockReturnValueOnce(new Promise<() => void>((resolve) => { resolveSubscription = resolve; }));
  await mount();
  await act(async () => root.unmount());
  await act(async () => resolveSubscription(disposeResize));
  expect(disposeResize).toHaveBeenCalledOnce();
  expect(disposeScale).toHaveBeenCalledOnce();
  expect(host.innerSize).not.toHaveBeenCalled();
  expect(vi.getTimerCount()).toBe(0);
});
