/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ACPMessageList } from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { AcpUiEventVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const writeText = vi.fn<(value: string) => Promise<void>>();

beforeEach(() => {
  writeText.mockResolvedValue();
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText },
  });
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
  writeText.mockReset();
  document.body.replaceChildren();
});

function event(overrides: Partial<AcpUiEventVm>): AcpUiEventVm {
  return {
    id: 'assistant-message',
    seq: 1,
    timestamp: '1Z',
    kind: 'textDelta',
    sessionId: 'session-1',
    content: '# 原始标题\n\n**加粗正文**',
    status: 'completed',
    raw: {},
    ...overrides,
  };
}

async function renderTimeline(
  timeline: AcpUiEventVm[],
  streamingMarkdownItemKey: string | null = null,
) {
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <TooltipProvider>
        <ACPMessageList
          timeline={timeline}
          sessionStatus="completed"
          sending={false}
          streamingMarkdownItemKey={streamingMarkdownItemKey}
        />
      </TooltipProvider>,
    );
  });
  return { container, root };
}

describe('completed Agent message Markdown copy action', () => {
  it('copies the canonical Markdown source and shows local success feedback', async () => {
    const markdown = '# 原始标题\n\n**加粗正文**';
    const { container, root } = await renderTimeline([event({ content: markdown })]);
    try {
      const button = container.querySelector<HTMLButtonElement>('[data-agent-message-copy="true"]');
      expect(button).not.toBeNull();
      expect(button?.getAttribute('aria-label')).toBe('复制 Markdown 源码');

      await act(async () => button?.click());

      expect(writeText).toHaveBeenCalledOnce();
      expect(writeText).toHaveBeenCalledWith(markdown);
      expect(button?.getAttribute('aria-label')).toBe('Markdown 源码已复制');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not expose the action for user, failed, empty, or non-message timeline items', async () => {
    const { container, root } = await renderTimeline([
      event({ id: 'user', seq: 1, kind: 'userTextDelta' }),
      event({ id: 'failed', seq: 2, status: 'failed' }),
      event({ id: 'empty', seq: 3, content: '   ' }),
      event({ id: 'tool', seq: 4, kind: 'toolCall', content: null, toolCallId: 'tool-1' }),
    ]);
    try {
      expect(container.querySelector('[data-agent-message-copy="true"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not expose the action while the Agent message is still streaming', async () => {
    const streamingEvent = event({ id: 'streaming', content: '仍在输出' });
    const { container, root } = await renderTimeline([streamingEvent], 'textDelta-streaming');
    try {
      expect(container.querySelector('[data-agent-message-copy="true"]')).toBeNull();
      expect(container.querySelector('[data-agent-quotable-text="true"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
