/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  alignChatContainerViewportToBottomBeforePaint,
  ChatContainerContent,
  ChatContainerRoot,
  type ChatContainerContext,
} from '@/components/prompt-kit/chat-container';
import {
  ConversationViewport,
  ConversationViewportFooter,
} from '@/components/conversation/ConversationViewport';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

class ControlledResizeObserver implements ResizeObserver {
  static instances: ControlledResizeObserver[] = [];

  readonly callback: ResizeObserverCallback;
  element: Element | null = null;
  elements = new Set<Element>();

  constructor(callback: ResizeObserverCallback) {
    this.callback = callback;
    ControlledResizeObserver.instances.push(this);
  }

  disconnect() {
    this.element = null;
    this.elements.clear();
  }

  observe(target: Element) {
    this.element = target;
    this.elements.add(target);
  }

  unobserve(target: Element) {
    if (this.element === target) this.element = null;
    this.elements.delete(target);
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
  it('keeps a dynamic footer outside streaming scroll content without remounting it', async () => {
    vi.stubGlobal('ResizeObserver', ControlledResizeObserver);
    const contextRef = React.createRef<ChatContainerContext>();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const view = (content: string) => React.createElement(
      ConversationViewport,
      {
        scrollClassName: 'overflow-y-auto',
        contextRef,
      },
      [
        React.createElement('div', { key: 'content', 'data-testid': 'streaming-content' }, content),
        React.createElement(
          ConversationViewportFooter,
          { key: 'footer' },
          React.createElement('div', { 'data-testid': 'dynamic-footer' },
            React.createElement('div', { 'data-conversation-viewport-overhang': true }, 'usage'),
            'composer'),
        ),
      ],
    );

    try {
      await act(async () => root.render(view('first chunk')));

      const viewport = contextRef.current?.scrollRef.current as HTMLDivElement | null;
      const content = contextRef.current?.contentRef.current as HTMLDivElement | null;
      const frame = container.querySelector<HTMLElement>('[data-conversation-viewport-frame="true"]');
      const footerLayer = container.querySelector<HTMLElement>('[data-conversation-viewport-footer="true"]');
      const footer = container.querySelector<HTMLElement>('[data-testid="dynamic-footer"]');

      expect(viewport).not.toBeNull();
      expect(content).not.toBeNull();
      expect(frame).not.toBeNull();
      expect(footerLayer).not.toBeNull();
      expect(footer).not.toBeNull();
      expect(viewport?.contains(footer)).toBe(false);
      expect(content?.style.paddingBottom).toBe(
        'var(--conversation-viewport-footer-height, 0px)',
      );

      const footerObserver = ControlledResizeObserver.instances.find(
        (observer) => observer.elements.has(footerLayer!),
      );
      expect(footerObserver).toBeDefined();
      const badge = footer!.querySelector<HTMLElement>('[data-conversation-viewport-overhang]')!;
      let badgeHeight = 24;
      let footerHeight = 96;
      vi.spyOn(footerLayer!, 'getBoundingClientRect').mockImplementation(() => (
        { top: 400 - footerHeight, bottom: 400, height: footerHeight } as DOMRect
      ));
      vi.spyOn(badge, 'getBoundingClientRect').mockImplementation(() => (
        { top: 400 - footerHeight - badgeHeight, bottom: 400 - footerHeight, height: badgeHeight } as DOMRect
      ));
      let scrollTop = 100;
      const availableHeight = () => 600 + Number.parseFloat(
        frame!.style.getPropertyValue('--conversation-viewport-footer-height'),
      );
      Object.defineProperties(viewport, {
        clientHeight: { configurable: true, value: 400 },
        scrollHeight: { configurable: true, get: availableHeight },
        scrollTop: { configurable: true, get: () => scrollTop, set: (value: number) => {
          scrollTop = Math.min(value, availableHeight() - 400);
        } },
      });
      vi.spyOn(viewport!, 'getBoundingClientRect').mockReturnValue({ top: 0, bottom: 400 } as DOMRect);
      const target = container.querySelector<HTMLElement>('[data-testid="streaming-content"]')!;
      vi.spyOn(target, 'getBoundingClientRect').mockReturnValue({ bottom: 500 } as DOMRect);
      await act(async () => {
        const token = contextRef.current!.beginContentExpansion();
        contextRef.current!.positionContentExpansion(token, target);
      });
      // Geometry is authoritative even before the footer observer publishes padding.
      expect(viewport!.scrollTop).toBe(320);
      await act(async () => { footerObserver?.emitHeight(96); await waitForScrollFrames(); });
      expect(frame?.style.getPropertyValue('--conversation-viewport-footer-height')).toBe('120px');
      expect(footerObserver?.elements.has(badge)).toBe(true);
      footerHeight = 160;
      badgeHeight = 32;
      await act(async () => { footerObserver?.emitHeight(32); await waitForScrollFrames(); });
      expect(frame?.style.getPropertyValue('--conversation-viewport-footer-height')).toBe('192px');
      badgeHeight = 0;
      footerHeight = 96;
      await act(async () => { footerObserver?.emitHeight(0); await waitForScrollFrames(); });
      expect(frame?.style.getPropertyValue('--conversation-viewport-footer-height')).toBe('96px');

      await act(async () => root.render(view('second streaming chunk')));
      expect(container.querySelector('[data-testid="dynamic-footer"]')).toBe(footer);
      expect(container.querySelector('[data-testid="streaming-content"]')?.textContent).toBe(
        'second streaming chunk',
      );
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('aligns the initial followed viewport before the first paint', () => {
    const viewport = {
      clientHeight: 320,
      scrollHeight: 1_120,
      scrollTop: 0,
    };

    alignChatContainerViewportToBottomBeforePaint(viewport);

    expect(viewport.scrollTop).toBe(800);
  });

  it('mounts a remembered manual conversation viewport without rejoining bottom follow', async () => {
    vi.stubGlobal('ResizeObserver', ControlledResizeObserver);
    const contextRef = React.createRef<ChatContainerContext>();
    const atBottomChanges: boolean[] = [];
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          React.createElement(
            ConversationViewport,
            {
              scrollClassName: 'overflow-y-auto',
              contextRef,
              initialFollowing: false,
              onAtBottomChange: (atBottom: boolean) => atBottomChanges.push(atBottom),
            },
            'remembered history',
          ),
        );
      });

      expect(contextRef.current?.isAtBottom).toBe(false);
      expect(atBottomChanges.at(-1)).toBe(false);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('reports only explicit wheel, keyboard, or scrollbar-pointer input as user scrolling', async () => {
    vi.stubGlobal('ResizeObserver', ControlledResizeObserver);
    const contextRef = React.createRef<ChatContainerContext>();
    const userScrolls: number[] = [];
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          React.createElement(
            ChatContainerRoot,
            { contextRef, onViewportUserScroll: () => userScrolls.push(1) },
            React.createElement(ChatContainerContent, null, 'streaming content'),
          ),
        );
      });
      const viewport = contextRef.current?.scrollRef.current as HTMLDivElement;

      await act(async () => {
        viewport.dispatchEvent(new Event('scroll'));
      });
      expect(userScrolls).toHaveLength(0);

      await act(async () => {
        viewport.dispatchEvent(new WheelEvent('wheel', { deltaY: -1 }));
        viewport.dispatchEvent(new KeyboardEvent('keydown', { key: 'PageUp' }));
        viewport.dispatchEvent(new Event('pointerdown', { bubbles: true }));
      });
      expect(userScrolls).toHaveLength(3);
    } finally {
      await act(async () => root.unmount());
    }
  });

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
        await vi.waitFor(() => {
          expect(scrollTop).toBe(139);
        }, { timeout: 5_000, interval: 10 });
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
        // A one-pixel upward move still lands inside the bottom tolerance,
        // but the explicit user escape must take precedence over geometry.
        scrollTop = 138;
        viewport?.dispatchEvent(new Event('scroll'));
        await waitForScrollFrames();
      });
      expect(contextRef.current?.isAtBottom).toBe(false);
      expect(atBottomChanges.at(-1)).toBe(false);

      contentHeight = 241;
      await act(async () => {
        emitObservedHeight(contentHeight);
        // Streaming layout and browser scroll anchoring can move the viewport
        // downward without any user intent to return to the latest message.
        scrollTop = 139;
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
      expect(scrollTop).toBe(139);
      expect(atBottomChanges.at(-1)).toBe(false);

      await act(async () => {
        viewport?.dispatchEvent(new WheelEvent('wheel', { deltaY: 1 }));
        scrollTop = 199;
        viewport?.dispatchEvent(new Event('scroll'));
        await waitForScrollFrames();
      });
      expect(atBottomChanges.at(-1)).toBe(false);

      await act(async () => {
        viewport?.dispatchEvent(new Event('scrollend'));
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

  it('compensates a prepended detail anchor without retriggering pagination in the same frame', async () => {
    vi.stubGlobal('ResizeObserver', ControlledResizeObserver);
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));

    const onViewportScroll = vi.fn();
    const contextRef = React.createRef<ChatContainerContext>();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          React.createElement(
            ChatContainerRoot,
            { contextRef, onViewportScroll },
            React.createElement(ChatContainerContent, null, 'bounded activity detail'),
          ),
        );
      });
      const viewport = contextRef.current?.scrollRef.current as HTMLDivElement;
      let scrollTop = 120;
      Object.defineProperties(viewport, {
        clientHeight: { configurable: true, get: () => 100 },
        scrollHeight: { configurable: true, get: () => 500 },
        scrollTop: {
          configurable: true,
          get: () => scrollTop,
          set: (value: number) => { scrollTop = Number(value); },
        },
      });
      onViewportScroll.mockClear();

      await act(async () => {
        expect(contextRef.current?.compensateContentAnchor(80)).toBe(true);
        viewport.dispatchEvent(new Event('scroll'));
      });
      expect(scrollTop).toBe(200);
      expect(onViewportScroll).not.toHaveBeenCalled();

      await act(async () => {
        await waitForScrollFrames();
        viewport.dispatchEvent(new Event('scroll'));
      });
      expect(onViewportScroll).toHaveBeenCalledOnce();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it.each([
    { newReply: false, userScroll: false, initiallyFollowing: true },
    { newReply: true, userScroll: false, initiallyFollowing: true },
    { newReply: false, userScroll: true, initiallyFollowing: true },
    { newReply: false, userScroll: false, initiallyFollowing: false },
  ])('resumes after collapse only if still at bottom: %j', async ({ newReply, userScroll, initiallyFollowing }) => {
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
            { resize: 'instant', initial: initiallyFollowing ? 'instant' : false, contextRef },
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

      contentHeight = newReply ? 300 : 240;
      await act(async () => {
        expect(contextRef.current?.endContentExpansion(expansionToken)).toBe(true);
        if (userScroll) viewport?.dispatchEvent(new WheelEvent('wheel', { deltaY: 1 }));
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(139);
      expect(contextRef.current?.isAtBottom).toBe(!newReply && !userScroll);

      contentHeight = 300;
      await act(async () => {
        emitObservedHeight(contentHeight);
        await waitForScrollFrames();
      });
      expect(scrollTop).toBe(newReply || userScroll ? 139 : 199);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('stays paused at geometric bottom until the last expansion closes', async () => {
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
      expect(contextRef.current?.isAtBottom).toBe(false);

      await act(async () => {
        expect(contextRef.current?.endContentExpansion(secondToken)).toBe(true);
        await waitForScrollFrames();
      });
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
