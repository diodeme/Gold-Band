/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const diagnosticRecords = vi.hoisted(() => [] as Array<{
  stage: string;
  details: Record<string, unknown>;
}>);

vi.mock('@/lib/acp-streaming-diagnostics', () => ({
  isAcpStreamingDiagnosticsEnabled: () => true,
  recordAcpStreamingDiagnostic: (
    stage: string,
    createDetails: () => Record<string, unknown>,
  ) => diagnosticRecords.push({ stage, details: createDetails() }),
}));

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
    if (!this.element) return;
    this.callback([{
      target: this.element,
      contentRect: { height },
    } as ResizeObserverEntry], this);
  }
}

afterEach(() => {
  diagnosticRecords.splice(0, diagnosticRecords.length);
  ControlledResizeObserver.instances = [];
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe('ChatContainer scroll diagnostics', () => {
  it('records the writer and geometry sequence around a manual escape', async () => {
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
            { contextRef, resize: 'instant', initial: 'instant' },
            React.createElement(
              ChatContainerContent,
              null,
              React.createElement('div', null, 'diagnostic fixture'),
            ),
          ),
        );
      });

      const viewport = contextRef.current?.scrollRef.current as HTMLDivElement;
      let contentHeight = 240;
      let scrollTop = 139;
      Object.defineProperties(viewport, {
        clientHeight: { configurable: true, get: () => 100 },
        scrollHeight: { configurable: true, get: () => contentHeight },
        scrollTop: {
          configurable: true,
          get: () => scrollTop,
          set: (value: number) => { scrollTop = Number(value); },
        },
      });

      await act(async () => {
        viewport.dispatchEvent(new WheelEvent('wheel', { deltaY: -1 }));
        scrollTop = 138;
        viewport.dispatchEvent(new Event('scroll'));
        contentHeight = 241;
        ControlledResizeObserver.instances.forEach((observer) => {
          observer.emitHeight(contentHeight);
        });
        contextRef.current?.scrollToBottom({ animation: 'instant' });
        await new Promise<void>((resolve) => window.setTimeout(resolve, 30));
      });

      const trace = diagnosticRecords.filter(
        (record) => record.stage === 'chat-scroll-trace',
      );
      const traceEvents = trace.map((record) => record.details.event);
      expect(traceEvents).toEqual(expect.arrayContaining([
        'wheel-up',
        'follow-write',
        'scroll',
        'content-resize',
        'scroll-to-bottom-call',
      ]));
      expect(trace.some((record) => (
        record.details.event === 'follow-write'
        && record.details.cause === 'user-wheel-up'
        && record.details.next === false
      ))).toBe(true);
      expect(trace.some((record) => (
        record.details.event === 'follow-write'
        && record.details.cause === 'external-scroll-to-bottom'
        && record.details.next === true
      ))).toBe(true);

      const instanceIds = new Set(trace.map((record) => record.details.instanceId));
      expect(instanceIds.size).toBe(1);
      expect(trace.every((record) => (
        !Object.hasOwn(record.details, 'content')
        && !Object.hasOwn(record.details, 'message')
      ))).toBe(true);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
