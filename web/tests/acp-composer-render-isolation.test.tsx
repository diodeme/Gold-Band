/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { streamdownRender } = vi.hoisted(() => ({ streamdownRender: vi.fn() }));

vi.mock('streamdown', () => ({
  defaultUrlTransform: (url: string) => url,
  parseMarkdownIntoBlocks: (markdown: string) => [markdown],
  Streamdown: ({ children }: { children: React.ReactNode }) => {
    streamdownRender();
    return <div>{children}</div>;
  },
}));

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, getAcpSession: vi.fn().mockResolvedValue(null) };
});

import { ACPChatDialog } from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { AcpSessionVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function completedSession(): AcpSessionVm {
  return {
    branchId: 'root',
    parentBranchId: null,
    readOnly: false,
    branchExecution: null,
    sessionId: 'composer-render-session',
    title: 'Composer render isolation',
    roundId: 'round-render',
    nodeId: 'node-render',
    attemptId: 'attempt-render',
    provider: 'test',
    status: 'completed',
    restored: false,
    events: [{
      id: 'assistant-message',
      seq: 1,
      timestamp: '1Z',
      kind: 'textDelta',
      sessionId: 'composer-render-session',
      content: 'historical **Markdown** message',
      title: null,
      toolCallId: null,
      status: 'completed',
      startedSeq: 1,
      endedSeq: 1,
      raw: {},
    }],
    eventPage: {
      loadedCount: 1,
      total: 1,
      oldestSeq: 1,
      newestSeq: 1,
      hasOlder: false,
      hasNewer: false,
      oldestCursor: null,
      newestCursor: null,
    },
    timelineProjection: { agents: [], todoEntries: [] },
    pendingPermissions: [],
    pendingElicitations: [],
    diagnostics: { rawFrameCount: 0, eventCount: 1, errorCount: 0 },
  };
}

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
    window.setTimeout(() => callback(performance.now()), 0)
  ));
  vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  streamdownRender.mockClear();
  document.body.replaceChildren();
});

describe('ACP composer render isolation', () => {
  it('keeps the conversation shell mounted when an established session payload is temporarily absent', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(
          <TooltipProvider>
            <ACPChatDialog
              session={null}
              sessionEstablished
              sessionReferenceId="persisted-session"
              projectId="project-render"
              taskId="task-render"
              runId="run-render"
              roundId="round-render"
              nodeId="node-render"
              attemptId="attempt-render"
              showSystemPromptAction={false}
              showRawFramesAction={false}
              usageCompact
            />
          </TooltipProvider>,
        );
      });

      expect(container.querySelector('textarea')).not.toBeNull();
      expect(container.textContent).not.toContain('ACP session failed');
      expect(container.textContent).not.toContain('ACP 会话失败');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps historical Markdown stable and measures textarea height once per input update', async () => {
    const scrollHeight = vi.spyOn(HTMLTextAreaElement.prototype, 'scrollHeight', 'get').mockReturnValue(72);
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(
          <TooltipProvider>
            <ACPChatDialog
              session={completedSession()}
              projectId="project-render"
              taskId="task-render"
              runId="run-render"
              roundId="round-render"
              nodeId="node-render"
              attemptId="attempt-render"
              showSystemPromptAction={false}
              showRawFramesAction={false}
              usageCompact
            />
          </TooltipProvider>,
        );
      });
      const initialMarkdownRenders = streamdownRender.mock.calls.length;
      expect(initialMarkdownRenders).toBe(1);
      scrollHeight.mockClear();

      const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
      expect(textarea).not.toBeNull();
      const valueSetter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set;
      await act(async () => {
        valueSetter?.call(textarea, 'a');
        textarea?.dispatchEvent(new Event('input', { bubbles: true }));
      });

      expect(textarea?.value).toBe('a');
      expect(streamdownRender).toHaveBeenCalledTimes(initialMarkdownRenders);
      expect(scrollHeight).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
