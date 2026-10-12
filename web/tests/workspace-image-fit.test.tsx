/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import '@/i18n';
import { TooltipProvider } from '@/components/ui/tooltip';
import { WorkspaceImageCanvas } from '@/components/workspace/files/WorkspaceImageCanvas';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

describe('image initial fit, reset and original size', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(600);
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(800);
    vi.spyOn(HTMLImageElement.prototype, 'naturalWidth', 'get').mockReturnValue(1365);
    vi.spyOn(HTMLImageElement.prototype, 'naturalHeight', 'get').mockReturnValue(900);
  });
  afterEach(() => {
    vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); document.body.replaceChildren();
  });

  it.each([true, false])('fits on first open and reset; 100% uses actual pixels (known size=%s)', async (knownSize) => {
    const container = document.createElement('div'); document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<TooltipProvider><WorkspaceImageCanvas src="image.png" alt="image"
        imageSize={knownSize ? { width: 1365, height: 900 } : undefined} /></TooltipProvider>));
      await act(async () => container.querySelector('img')!.dispatchEvent(new Event('load')));
      const label = () => container.querySelector('[data-workspace-image-canvas] > div:last-child > span:last-child')!.textContent;
      expect(label()).toBe('41%');
      const original = container.querySelector<HTMLButtonElement>('button[aria-label="原始大小"]')!;
      expect(original.textContent).toBe('100%');
      await act(async () => original.click());
      await act(async () => vi.advanceTimersByTime(300));
      expect(label()).toBe('100%');
      expect(container.querySelector<HTMLElement>('.react-transform-component')!.style.transform).toContain('scale(1)');
      // A repeated load (e.g. refreshed URL) must retain the chosen size.
      await act(async () => container.querySelector('img')!.dispatchEvent(new Event('load')));
      expect(label()).toBe('100%');
      expect(container.querySelector('button[aria-label="重置图片"]')).toBeNull();
      expect(container.querySelectorAll('button[aria-label="适应窗口"]')).toHaveLength(1);
      const fit = container.querySelector<HTMLButtonElement>('button[aria-label="适应窗口"]')!;
      expect(fit.getAttribute('data-slot')).toBe('tooltip-trigger');
      await act(async () => fit.focus());
      expect(document.querySelector('[role="tooltip"]')?.textContent).toBe('适应窗口');
      await act(async () => fit.click());
      await act(async () => vi.advanceTimersByTime(300));
      expect(label()).toBe('41%');
    } finally { await act(async () => root.unmount()); }
  });
});
