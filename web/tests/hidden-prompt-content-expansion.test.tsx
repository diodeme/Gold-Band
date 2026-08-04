/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { HiddenPromptMessageContent } from '@/components/acp/HiddenPromptMessageContent';
import {
  ChatContainerContent,
  ChatContainerRoot,
  type ChatContainerContext,
} from '@/components/prompt-kit/chat-container';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

describe('hidden prompt content expansion', () => {
  it('uses the shared chat disclosure lifecycle when opening and closing', async () => {
    vi.stubGlobal('ResizeObserver', class {
      observe() {}
      unobserve() {}
      disconnect() {}
    });
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));
    Object.defineProperty(Range.prototype, 'getClientRects', {
      configurable: true,
      value: () => [],
    });

    const contextRef = React.createRef<ChatContainerContext>();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const content = [
      '<hidden data-gold-band-hidden="true" title="Gold Band runtime context">',
      'runtime detail',
      '</hidden>',
      '# Requirement',
      'verify disclosure scrolling',
    ].join('\n');

    try {
      await act(async () => {
        root.render(
          <ChatContainerRoot contextRef={contextRef} resize="instant" initial="instant">
            <ChatContainerContent scrollClassName="overflow-y-auto">
              <HiddenPromptMessageContent content={content} />
            </ChatContainerContent>
          </ChatContainerRoot>,
        );
      });

      const trigger = container.querySelector<HTMLButtonElement>(
        '[data-slot="collapsible-trigger"]',
      );
      expect(trigger).not.toBeNull();

      await act(async () => {
        trigger?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      expect(trigger?.getAttribute('aria-expanded')).toBe('true');
      expect(contextRef.current?.isAtBottom).toBe(false);

      await act(async () => {
        trigger?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        await new Promise((resolve) => window.setTimeout(resolve, 5));
      });
      expect(trigger?.getAttribute('aria-expanded')).toBe('false');
      expect(contextRef.current?.isAtBottom).toBe(true);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
