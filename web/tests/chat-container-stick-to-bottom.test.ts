/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  ChatContainerContent,
  ChatContainerRoot,
  type ChatContainerContext,
} from '@/components/prompt-kit/chat-container';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

class ControlledResizeObserver implements ResizeObserver {
  static instances: ControlledResizeObserver[] = [];

  readonly callback: ResizeObserverCallback;
  element: Element | null = null;

  constructor(callback: ResizeObserverCallback) {
    this.callback = callback;
    ControlledResizeObserver.instances.push(this);
  }

  disconnect() {
    this.element = null;
  }

  observe(target: Element) {
    this.element = target;
  }

  unobserve(target: Element) {
    if (this.element === target) this.element = null;
  }

  emitHeight(height: number) {
    if (!this.element) throw new Error('ResizeObserver has no observed element');
    this.callback([
      {
        target: this.element,
        contentRect: { height },
      } as ResizeObserverEntry,
    ], this);
  }
}

function waitForScrollFrames() {
  return new Promise<void>((resolve) => window.setTimeout(resolve, 12));
}

afterEach(() => {
  ControlledResizeObserver.instances = [];
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

describe('prompt-kit ChatContainer stick-to-bottom lifecycle', () => {
  it('follows real content resizes, preserves a manual reading position, and resumes after returning to bottom', async () => {
    vi.stubGlobal('ResizeObserver', ControlledResizeObserver);
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));

    const atBottomChanges: boolean[] = [];
    const contextRef = React.createRef<ChatContainerContext>();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          React.createElement(
            ChatContainerRoot,
            {
              className: 'h-full',
              resize: 'instant',
              initial: 'instant',
              contextRef,
              onAtBottomChange: (atBottom) => atBottomChanges.push(atBottom),
            },
            React.createElement(
              ChatContainerContent,
              { scrollClassName: 'overflow-y-auto' },
              React.createElement('div', null, 'streaming content'),
            ),
          ),
        );
      });

      const context = contextRef.current;
      const viewport = context?.scrollRef.current as HTMLDivElement | null;
      const observer = ControlledResizeObserver.instances.at(-1);
      expect(context).toBeDefined();
      expect(viewport).not.toBeNull();
      expect(observer).toBeDefined();

      let contentHeight = 100;
      let scrollTop = 0;
      Object.defineProperties(viewport, {
        clientHeight: { configurable: true, get: () => 100 },
        scrollHeight: { configurable: true, get: () => contentHeight },
        scrollTop: {
          configurable: true,
          get: () => scrollTop,
          set: (value: number) => {
            scrollTop = Number(value);
          },
        },
      });

      await act(async () => {
        observer?.emitHeight(contentHeight);
        await waitForScrollFrames();
      });

      contentHeight = 240;
      await act(async () => {
        observer?.emitHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(139);

      await act(async () => {
        context?.stopScroll();
      });
      scrollTop = 60;
      viewport?.dispatchEvent(new Event('scroll'));
      contentHeight = 300;
      await act(async () => {
        observer?.emitHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(60);
      expect(atBottomChanges.at(-1)).toBe(false);

      await act(async () => {
        scrollTop = 199;
        viewport?.dispatchEvent(new Event('scroll'));
        await waitForScrollFrames();
      });
      expect(atBottomChanges.at(-1)).toBe(true);

      await act(async () => {
        context?.stopScroll();
      });
      contentHeight = 360;
      await act(async () => {
        observer?.emitHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(199);

      await act(async () => {
        scrollTop = 259;
        viewport?.dispatchEvent(new Event('scroll'));
        await waitForScrollFrames();
      });
      expect(atBottomChanges.at(-1)).toBe(true);

      contentHeight = 420;
      await act(async () => {
        observer?.emitHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(319);
    } finally {
      await act(async () => {
        root.unmount();
      });
    }
  });
});
