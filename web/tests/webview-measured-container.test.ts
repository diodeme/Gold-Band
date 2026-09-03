// @vitest-environment jsdom

import { describe, expect, it, vi } from 'vitest';
import {
  measuredContainerTierTokens,
  observeMeasuredWebviewContainer,
  type MeasuredContainerEnvironment,
} from '@/lib/webview-measured-container';

class ControlledResizeObserver implements ResizeObserver {
  static instance: ControlledResizeObserver | null = null;
  private target: Element | null = null;

  constructor(private readonly callback: ResizeObserverCallback) {
    ControlledResizeObserver.instance = this;
  }

  observe(target: Element) { this.target = target; }
  unobserve() {}
  disconnect() { this.target = null; }

  publish(width: number) {
    if (!this.target) throw new Error('No measured container target');
    this.callback([{
      target: this.target,
      contentRect: { width } as DOMRectReadOnly,
    } as ResizeObserverEntry], this);
  }
}

function controlledEnvironment() {
  let callback: FrameRequestCallback | null = null;
  const environment: MeasuredContainerEnvironment = {
    ResizeObserver: ControlledResizeObserver,
    requestAnimationFrame(next) {
      callback = next;
      return 1;
    },
    cancelAnimationFrame: vi.fn(() => { callback = null; }),
  };
  return {
    environment,
    flush() {
      const pending = callback;
      callback = null;
      pending?.(0);
    },
  };
}

describe('measured WebView container adapter', () => {
  it('maps Tailwind container breakpoints to cumulative discrete tiers', () => {
    expect(measuredContainerTierTokens(319)).toBe('');
    expect(measuredContainerTierTokens(576)).toBe('xs sm md lg xl');
    expect(measuredContainerTierTokens(1152)).toBe('xs sm md lg xl 2xl 3xl 4xl 5xl 6xl');
  });

  it('publishes at most once per animation frame and only changes discrete tiers', () => {
    const element = document.createElement('div');
    vi.spyOn(element, 'getBoundingClientRect').mockReturnValue({ width: 300 } as DOMRect);
    const { environment, flush } = controlledEnvironment();
    const dispose = observeMeasuredWebviewContainer(element, 'settings-content', environment);

    flush();
    expect(element.dataset).toMatchObject({
      webviewContainer: 'settings-content',
      webviewContainerTiers: '',
    });

    ControlledResizeObserver.instance?.publish(580);
    ControlledResizeObserver.instance?.publish(700);
    expect(element.dataset.webviewContainerTiers).toBe('');
    flush();
    expect(element.dataset.webviewContainerTiers).toBe('xs sm md lg xl 2xl');

    ControlledResizeObserver.instance?.publish(701);
    flush();
    expect(element.dataset.webviewContainerTiers).toBe('xs sm md lg xl 2xl');

    dispose();
    expect(element.dataset.webviewContainer).toBeUndefined();
    expect(element.dataset.webviewContainerTiers).toBeUndefined();
  });
});
