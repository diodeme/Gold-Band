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
  return new Promise<void>((resolve) => window.setTimeout(resolve, 24));
}

function waitForFollowRecovery() {
  return new Promise<void>((resolve) => window.setTimeout(resolve, 80));
}

function emitObservedHeight(height: number) {
  for (const observer of ControlledResizeObserver.instances) {
    if (observer.element) observer.emitHeight(height);
  }
}

afterEach(() => {
  ControlledResizeObserver.instances = [];
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

describe('prompt-kit ChatContainer stick-to-bottom lifecycle', () => {
  it('restores bottom following when a layout scroll races ahead of content resize observation', async () => {
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
      expect(context).toBeDefined();
      expect(viewport).not.toBeNull();
      expect(ControlledResizeObserver.instances.length).toBeGreaterThanOrEqual(2);

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
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });

      contentHeight = 240;
      await act(async () => {
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(139);

      // Chromium can publish a layout-driven scroll event before the matching
      // ResizeObserver notification. The dependency briefly interprets that
      // upward movement as a user escape, but the wrapper still owns follow intent.
      await act(async () => {
        scrollTop = 96;
        viewport?.dispatchEvent(new Event('scroll'));
        await waitForFollowRecovery();
      });
      expect(scrollTop).toBe(139);
      expect(contextRef.current?.state.isAtBottom).toBe(true);
      expect(atBottomChanges).not.toContain(false);

      contentHeight = 300;
      await act(async () => {
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(199);

      // The turn file-change card first mounts, then grows again when its file
      // details arrive. Both layout phases must remain part of the same follow.
      contentHeight = 360;
      await act(async () => {
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(259);
      expect(contextRef.current?.isAtBottom).toBe(true);
    } finally {
      await act(async () => {
        root.unmount();
      });
    }
  });

  it('preserves a wheel reading position and resumes after returning to bottom', async () => {
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

      const viewport = contextRef.current?.scrollRef.current as HTMLDivElement | null;
      expect(viewport).not.toBeNull();

      let contentHeight = 240;
      let scrollTop = 139;
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
        emitObservedHeight(contentHeight);
        viewport?.dispatchEvent(new WheelEvent('wheel', { deltaY: -1 }));
        scrollTop = 130;
        viewport?.dispatchEvent(new Event('scroll'));
        await waitForScrollFrames();
      });
      expect(contextRef.current?.isAtBottom).toBe(false);
      expect(atBottomChanges.at(-1)).toBe(false);

      contentHeight = 300;
      await act(async () => {
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(130);
      expect(atBottomChanges.at(-1)).toBe(false);

      await act(async () => {
        scrollTop = 199;
        viewport?.dispatchEvent(new Event('scroll'));
        await waitForScrollFrames();
      });
      expect(atBottomChanges.at(-1)).toBe(true);

      contentHeight = 360;
      await act(async () => {
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(259);
    } finally {
      await act(async () => {
        root.unmount();
      });
    }
  });

  it('treats an external stopScroll call as an intentional manual position', async () => {
    vi.stubGlobal('ResizeObserver', ControlledResizeObserver);
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));

    const contextRef = React.createRef<ChatContainerContext>();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          React.createElement(
            ChatContainerRoot,
            { resize: 'instant', initial: 'instant', contextRef },
            React.createElement(
              ChatContainerContent,
              { scrollClassName: 'overflow-y-auto' },
              React.createElement('div', null, 'paginated content'),
            ),
          ),
        );
      });

      const viewport = contextRef.current?.scrollRef.current as HTMLDivElement | null;
      let contentHeight = 300;
      let scrollTop = 120;
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
        contextRef.current?.stopScroll();
      });
      contentHeight = 420;
      await act(async () => {
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(120);
      expect(contextRef.current?.isAtBottom).toBe(false);
    } finally {
      await act(async () => {
        root.unmount();
      });
    }
  });

  it('lets bottom content expand downward and restores following after it collapses', async () => {
    vi.stubGlobal('ResizeObserver', ControlledResizeObserver);
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));

    const contextRef = React.createRef<ChatContainerContext>();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          React.createElement(
            ChatContainerRoot,
            { resize: 'instant', initial: 'instant', contextRef },
            React.createElement(
              ChatContainerContent,
              { scrollClassName: 'overflow-y-auto' },
              React.createElement('div', null, 'expandable activity'),
            ),
          ),
        );
      });

      const viewport = contextRef.current?.scrollRef.current as HTMLDivElement | null;
      let contentHeight = 240;
      let scrollTop = 139;
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

      let expansionToken: number | null = null;
      await act(async () => {
        emitObservedHeight(contentHeight);
        expansionToken = contextRef.current?.beginContentExpansion() ?? null;
        contentHeight = 360;
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(expansionToken).not.toBeNull();
      expect(scrollTop).toBe(139);
      expect(contextRef.current?.isAtBottom).toBe(false);

      contentHeight = 240;
      await act(async () => {
        expect(contextRef.current?.endContentExpansion(expansionToken)).toBe(true);
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(139);
      expect(contextRef.current?.isAtBottom).toBe(true);

      contentHeight = 300;
      await act(async () => {
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(199);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('resumes following when collapsing one of several expansions reaches the bottom', async () => {
    vi.stubGlobal('ResizeObserver', ControlledResizeObserver);
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));

    const contextRef = React.createRef<ChatContainerContext>();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          React.createElement(
            ChatContainerRoot,
            { resize: 'instant', initial: 'instant', contextRef },
            React.createElement(
              ChatContainerContent,
              { scrollClassName: 'overflow-y-auto' },
              React.createElement('div', null, 'several expandable activities'),
            ),
          ),
        );
      });

      const viewport = contextRef.current?.scrollRef.current as HTMLDivElement | null;
      let contentHeight = 240;
      let scrollTop = 139;
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

      let firstToken: number | null = null;
      let secondToken: number | null = null;
      await act(async () => {
        emitObservedHeight(contentHeight);
        firstToken = contextRef.current?.beginContentExpansion() ?? null;
        contentHeight = 320;
        emitObservedHeight(contentHeight);
        secondToken = contextRef.current?.beginContentExpansion() ?? null;
        contentHeight = 420;
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(139);
      expect(contextRef.current?.isAtBottom).toBe(false);

      await act(async () => {
        expect(contextRef.current?.endContentExpansion(firstToken)).toBe(false);
        contentHeight = 240;
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(139);
      expect(contextRef.current?.isAtBottom).toBe(true);

      expect(contextRef.current?.endContentExpansion(secondToken)).toBe(false);
      contentHeight = 300;
      await act(async () => {
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(199);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not restore disclosure following after the user scrolls while expanded', async () => {
    vi.stubGlobal('ResizeObserver', ControlledResizeObserver);
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));

    const contextRef = React.createRef<ChatContainerContext>();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          React.createElement(
            ChatContainerRoot,
            { resize: 'instant', initial: 'instant', contextRef },
            React.createElement(
              ChatContainerContent,
              { scrollClassName: 'overflow-y-auto' },
              React.createElement('div', null, 'expandable activity'),
            ),
          ),
        );
      });

      const viewport = contextRef.current?.scrollRef.current as HTMLDivElement | null;
      let contentHeight = 240;
      let scrollTop = 139;
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

      let expansionToken: number | null = null;
      await act(async () => {
        emitObservedHeight(contentHeight);
        expansionToken = contextRef.current?.beginContentExpansion() ?? null;
        contentHeight = 360;
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
        viewport?.dispatchEvent(new WheelEvent('wheel', { deltaY: -20 }));
        scrollTop = 50;
        viewport?.dispatchEvent(new Event('scroll'));
        await waitForScrollFrames();
      });

      contentHeight = 240;
      await act(async () => {
        expect(contextRef.current?.endContentExpansion(expansionToken)).toBe(false);
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(50);
      expect(contextRef.current?.isAtBottom).toBe(false);

      contentHeight = 300;
      await act(async () => {
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(50);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps the bottom lock across an approval-card collapse followed by the next approval card', async () => {
    vi.stubGlobal('ResizeObserver', ControlledResizeObserver);
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));

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
            },
            React.createElement(
              ChatContainerContent,
              { scrollClassName: 'overflow-y-auto' },
              React.createElement('div', null, 'expanded activity'),
            ),
          ),
        );
      });

      const context = contextRef.current;
      const viewport = context?.scrollRef.current as HTMLDivElement | null;
      expect(context).toBeDefined();
      expect(viewport).not.toBeNull();

      let contentHeight = 520;
      let scrollTop = 419;
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
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });

      // The answered card is replaced by its compact audit row. Browsers clamp
      // scrollTop before ResizeObserver reports the smaller content height.
      contentHeight = 420;
      await act(async () => {
        scrollTop = 319;
        viewport?.dispatchEvent(new Event('scroll'));
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });

      // Tool output grows and the next pending approval card is mounted.
      contentHeight = 660;
      await act(async () => {
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(559);
      expect(context?.state.isAtBottom).toBe(true);

      await act(async () => {
        viewport?.dispatchEvent(new WheelEvent('wheel', { deltaY: -1 }));
        await waitForScrollFrames();
      });
      contentHeight = 760;
      await act(async () => {
        scrollTop = 500;
        viewport?.dispatchEvent(new Event('scroll'));
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(500);
      expect(context?.state.isAtBottom).toBe(false);
    } finally {
      await act(async () => {
        root.unmount();
      });
    }
  });
});
