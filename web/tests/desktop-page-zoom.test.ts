import { readFileSync } from 'node:fs';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  installDesktopPageZoomGuard,
  isDesktopPageZoomShortcut,
  setDesktopPageZoomDriver,
} from '@/lib/desktop-page-zoom';

function createFakeTarget() {
  const listeners = new Map<string, Array<(event: unknown) => void>>();
  return {
    addEventListener(type: string, listener: (event: unknown) => void) {
      listeners.set(type, [...(listeners.get(type) ?? []), listener]);
    },
    removeEventListener(type: string, listener: (event: unknown) => void) {
      listeners.set(type, (listeners.get(type) ?? []).filter((item) => item !== listener));
    },
    dispatch(type: string, event: Record<string, unknown>) {
      let prevented = false;
      const payload = { preventDefault: () => { prevented = true; }, ...event };
      for (const listener of listeners.get(type) ?? []) listener(payload);
      return prevented;
    },
  };
}

describe('desktop page zoom boundary', () => {
  it('enables the Windows WebView2 pinch input before the webview is built', () => {
    const source = readFileSync(path.resolve(__dirname, '../../src-tauri/src/main.rs'), 'utf8');

    expect(source).toContain('window.zoom_hotkeys_enabled = true;');
  });

  it('prevents page zoom while preserving ordinary keyboard input', () => {
    expect(isDesktopPageZoomShortcut('+', true, false)).toBe(true);
    expect(isDesktopPageZoomShortcut('=', true, false)).toBe(true);
    expect(isDesktopPageZoomShortcut('-', false, true)).toBe(true);
    expect(isDesktopPageZoomShortcut('0', true, false)).toBe(true);
    expect(isDesktopPageZoomShortcut('0', false, false)).toBe(false);
    expect(isDesktopPageZoomShortcut('a', true, false)).toBe(false);
  });

  it('installs one capture guard without stopping image viewport propagation', () => {
    const source = readFileSync(path.resolve(__dirname, '../src/lib/desktop-page-zoom.ts'), 'utf8');

    expect(source).toContain("addEventListener('wheel', preventPageWheelZoom, { capture: true, passive: false })");
    expect(source).toContain('if (!event.ctrlKey) return;');
    expect(source).toContain('event.preventDefault();');
    expect(source).not.toContain('stopPropagation');
  });
});

describe('desktop page zoom driver', () => {
  afterEach(() => {
    setDesktopPageZoomDriver(null);
  });

  it('forwards ctrl+wheel steps to the installed driver and always consumes the gesture', () => {
    const target = createFakeTarget();
    const dispose = installDesktopPageZoomGuard(target as unknown as Window);
    try {
      // Without a driver the native gesture is still consumed.
      expect(target.dispatch('wheel', { ctrlKey: true, deltaY: -100 })).toBe(true);

      const adjust = vi.fn();
      setDesktopPageZoomDriver({ adjust, reset: vi.fn() });
      expect(target.dispatch('wheel', { ctrlKey: true, deltaY: -100 })).toBe(true);
      expect(target.dispatch('wheel', { ctrlKey: true, deltaY: 100 })).toBe(true);
      expect(adjust).toHaveBeenNthCalledWith(1, 5);
      expect(adjust).toHaveBeenNthCalledWith(2, -5);
      // Plain wheel scrolling is untouched.
      expect(target.dispatch('wheel', { ctrlKey: false, deltaY: 100 })).toBe(false);
      expect(adjust).toHaveBeenCalledTimes(2);
    } finally {
      dispose();
    }
  });

  it('accumulates pinch-sized ctrl+wheel deltas so one gesture steps once per threshold', () => {
    const target = createFakeTarget();
    const dispose = installDesktopPageZoomGuard(target as unknown as Window);
    try {
      const adjust = vi.fn();
      setDesktopPageZoomDriver({ adjust, reset: vi.fn() });
      // Precision-touchpad pinches synthesize bursts of tiny pixel deltas; each
      // event alone must not step, only the accumulated distance may.
      for (let index = 0; index < 7; index += 1) {
        expect(target.dispatch('wheel', { ctrlKey: true, deltaY: -10, deltaMode: 0 })).toBe(true);
      }
      expect(adjust).not.toHaveBeenCalled();
      expect(target.dispatch('wheel', { ctrlKey: true, deltaY: -10, deltaMode: 0 })).toBe(true);
      expect(adjust).toHaveBeenCalledTimes(1);
      expect(adjust).toHaveBeenNthCalledWith(1, 5);

      // Line-mode notches (physical mouse wheels) still step once per notch.
      expect(target.dispatch('wheel', { ctrlKey: true, deltaY: -3, deltaMode: 1 })).toBe(true);
      expect(adjust).toHaveBeenNthCalledWith(2, 5);

      // A zero delta never steps in either direction.
      expect(target.dispatch('wheel', { ctrlKey: true, deltaY: 0, deltaMode: 0 })).toBe(true);
      expect(adjust).toHaveBeenCalledTimes(2);
    } finally {
      dispose();
    }
  });

  it('maps keyboard shortcuts to step and reset intents', () => {
    const target = createFakeTarget();
    const dispose = installDesktopPageZoomGuard(target as unknown as Window);
    try {
      const adjust = vi.fn();
      const reset = vi.fn();
      setDesktopPageZoomDriver({ adjust, reset });

      expect(target.dispatch('keydown', { key: '=', ctrlKey: true, metaKey: false })).toBe(true);
      expect(target.dispatch('keydown', { key: '+', ctrlKey: false, metaKey: true })).toBe(true);
      expect(target.dispatch('keydown', { key: '-', ctrlKey: true, metaKey: false })).toBe(true);
      expect(target.dispatch('keydown', { key: '0', ctrlKey: true, metaKey: false })).toBe(true);
      expect(target.dispatch('keydown', { key: 'a', ctrlKey: true, metaKey: false })).toBe(false);

      expect(adjust).toHaveBeenNthCalledWith(1, 5);
      expect(adjust).toHaveBeenNthCalledWith(2, 5);
      expect(adjust).toHaveBeenNthCalledWith(3, -5);
      expect(reset).toHaveBeenCalledTimes(1);
      expect(adjust).toHaveBeenCalledTimes(3);
    } finally {
      dispose();
    }
  });
});
