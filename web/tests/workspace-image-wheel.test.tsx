/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import '@/i18n';
import { TooltipProvider } from '@/components/ui/tooltip';
import { WorkspaceImageCanvas } from '@/components/workspace/files/WorkspaceImageCanvas';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

describe('workspace image wheel gestures', () => {
  it.each([false, true])('retains fractional high-frequency deltas with one transform per frame (ctrlKey=%s)', async (ctrlKey) => {
    vi.useFakeTimers();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onViewStateChange = vi.fn();
    try {
      await act(async () => root.render(<TooltipProvider><WorkspaceImageCanvas src="test.png" alt="test" onViewStateChange={onViewStateChange} /></TooltipProvider>));
      const viewport = container.querySelector('[data-workspace-image-viewport]')!;
      for (let frame = 0; frame < 10; frame += 1) {
        for (let event = 0; event < 4; event += 1) {
          viewport.dispatchEvent(new WheelEvent('wheel', { bubbles: true, cancelable: true, ctrlKey, deltaY: -0.25 }));
        }
        await act(async () => vi.advanceTimersToNextFrame());
        expect(onViewStateChange).toHaveBeenCalledTimes(frame + 1);
        expect(onViewStateChange.mock.lastCall![0].scale).toBeCloseTo(Math.exp((frame + 1) * 0.003), 6);
      }
    } finally { await act(async () => root.unmount()); }
  });

  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.replaceChildren();
  });

  it.each([false, true])('zooms at the pointer in both directions (ctrlKey=%s)', async (ctrlKey) => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<TooltipProvider><WorkspaceImageCanvas src="test.png" alt="test" /></TooltipProvider>));
      const viewport = container.querySelector('[data-workspace-image-viewport]')!;
      const content = container.querySelector('.react-transform-component') as HTMLElement;
      const label = container.querySelector('[data-workspace-image-canvas] > div:last-child > span:last-child')!;
      const wheel = (deltaY: number) => {
        const event = new WheelEvent('wheel', {
          bubbles: true, cancelable: true, ctrlKey, deltaY, clientX: 100, clientY: 80,
        });
        viewport.dispatchEvent(event);
        return event;
      };

      // Two inputs in the same frame must accumulate, then update the real library DOM.
      wheel(-50);
      wheel(-50);
      expect(label.textContent).toBe('100%');
      await act(async () => vi.advanceTimersToNextFrame());
      expect(label.textContent).toBe('135%');
      expect(content.style.transform).toContain('scale(1.349858');
      expect(content.style.transform).toContain('translate(-34.9858');

      const zoomOut = wheel(100);
      await act(async () => vi.advanceTimersToNextFrame());
      expect(label.textContent).toBe('100%');
      expect(zoomOut.defaultPrevented).toBe(true);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
