/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const runtime = vi.hoisted(() => ({
  listener: null as ((event: unknown) => void) | null,
}));

vi.mock('@/api/client', async () => {
  const actual = await vi.importActual<typeof import('@/api/client')>('@/api/client');
  return {
    ...actual,
    getRuntimeApi: () => ({
      subscribeAcpSessionUpdates: async (listener: (event: unknown) => void) => {
        runtime.listener = listener;
        return () => undefined;
      },
      getSupportedAttachmentExtensions: async () => [],
    }),
  };
});

vi.mock('@tauri-apps/api/event', () => ({
  listen: async () => () => undefined,
}));

vi.mock('@/api/shared', async () => {
  const actual = await vi.importActual<typeof import('@/api/shared')>('@/api/shared');
  return { ...actual, isTauriRuntime: () => true };
});

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, getAcpSession: vi.fn() };
});

vi.mock('@/components/prompt-kit/markdown', () => ({
  Markdown: ({ children, streaming }: { children: React.ReactNode; streaming?: boolean }) => (
    <div data-testid="markdown" data-streaming={streaming ? 'true' : 'false'}>{children}</div>
  ),
}));

import { getAcpSession } from '@/api';
import { ACPChatDialog } from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import {
  applyConversationEventToBranchSnapshots,
  CONVERSATION_EVENT_REPLAY_LIMITS,
  resetConversationEventRouterSnapshots,
} from '@/lib/conversation-event-router';
import type { AcpSessionUpdatedEventVm } from '@/api/client';
import type { AcpSessionVm, AcpUiEventVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const locator = {
  projectId: 'project-watermark',
  taskId: 'task-watermark',
  runId: 'run-watermark',
  roundId: 'round-watermark',
  nodeId: 'node-watermark',
  attemptId: 'attempt-watermark',
};

function event(
  id: string,
  seq: number,
  kind: string,
  content: string | null = null,
  extra: Partial<AcpUiEventVm> = {},
): AcpUiEventVm {
  return {
    id,
    seq,
    timestamp: `${seq}Z`,
    kind,
    sessionId: 'session-watermark',
    content,
    title: null,
    toolCallId: null,
    status: null,
    startedSeq: seq,
    endedSeq: seq,
    raw: null,
    ...extra,
  };
}

function session(events: AcpUiEventVm[], status = 'running'): AcpSessionVm {
  const oldestSeq = events[0]?.seq ?? null;
  const newestSeq = events.at(-1)?.endedSeq ?? events.at(-1)?.seq ?? null;
  return {
    branchId: 'root',
    parentBranchId: null,
    readOnly: false,
    sessionId: 'session-watermark',
    roundId: locator.roundId,
    nodeId: locator.nodeId,
    attemptId: locator.attemptId,
    provider: 'test',
    status,
    restored: false,
    events,
    eventPage: {
      generation: 1,
      coveredRevision: newestSeq ?? 0,
      newestRevision: newestSeq,
      loadedCount: events.length,
      total: events.length,
      oldestSeq,
      newestSeq,
      hasOlder: false,
      hasNewer: false,
    },
    timelineProjection: { agents: [], todoEntries: [] },
    pendingPermissions: [],
    pendingElicitations: [],
    diagnostics: { rawFrameCount: 0, eventCount: events.length, errorCount: 0 },
  };
}

function update(eventUpdate: AcpUiEventVm): AcpSessionUpdatedEventVm {
  return {
    ...locator,
    branchId: 'root',
    timelineGeneration: 1,
    timelineRevision: eventUpdate.endedSeq ?? eventUpdate.seq,
    event: eventUpdate,
  };
}

async function renderDialog(
  acpSession: AcpSessionVm,
  branchId = 'root',
  onInitialSessionQueryStateChange?: (state: 'loading' | 'success' | 'error') => void,
) {
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <TooltipProvider>
        <ACPChatDialog
          session={acpSession}
          {...locator}
          branchId={branchId}
          onInitialSessionQueryStateChange={onInitialSessionQueryStateChange}
          showSystemPromptAction={false}
          showRawFramesAction={false}
          usageCompact
        />
      </TooltipProvider>,
    );
    await new Promise((resolve) => window.setTimeout(resolve, 0));
  });
  return { container, root };
}

async function unmount(root: Root) {
  await act(async () => root.unmount());
}

beforeEach(() => {
  resetConversationEventRouterSnapshots();
  vi.mocked(getAcpSession).mockReset();
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
    window.setTimeout(() => callback(performance.now()), 0)
  ));
  vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));
  if (!Range.prototype.getClientRects) {
    Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
  }
});

afterEach(() => {
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe('ACP session re-entry reconciliation', () => {
  it('closes the initial query gate on a ready live session and ignores a late placeholder', async () => {
    const placeholder = {
      ...session([], 'pending'),
      sessionId: null,
    };
    const prompt = event('prompt-live-ready', 1, 'userTextDelta', '首轮已经就绪', {
      raw: { source: 'goldBandPrompt', promptId: 'prompt-live-ready' },
    });
    const ready = session([prompt]);
    ready.eventPage.generation = 2;
    let resolveInitialFetch: ((value: AcpSessionVm) => void) | null = null;
    vi.mocked(getAcpSession).mockImplementationOnce(() => new Promise((resolve) => {
      resolveInitialFetch = resolve;
    }));
    const queryStates: string[] = [];

    const { container, root } = await renderDialog(
      placeholder,
      'root',
      (state) => queryStates.push(state),
    );
    try {
      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          timelineGeneration: 2,
          timelineRevision: 1,
          session: ready,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      expect(container.textContent).toContain('首轮已经就绪');
      expect(queryStates.at(-1)).toBe('success');

      await act(async () => {
        resolveInitialFetch?.(placeholder);
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      expect(container.textContent).toContain('首轮已经就绪');
      expect(queryStates.at(-1)).toBe('success');
      expect(queryStates).not.toContain('error');
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(1);
    } finally {
      await unmount(root);
    }
  });

  it('keeps retrying when the first response is an unmaterialized control placeholder', async () => {
    const placeholder = {
      ...session([], 'pending'),
      sessionId: null,
    };
    const prompt = event('prompt-ready', 1, 'userTextDelta', '会话已经就绪', {
      raw: { source: 'goldBandPrompt', promptId: 'prompt-ready' },
    });
    const ready = session([prompt]);
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(placeholder)
      .mockResolvedValue(ready);

    const { container, root } = await renderDialog(placeholder);
    try {
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(1);
      expect(container.textContent).not.toContain('会话已经就绪');

      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 160));
      });

      expect(vi.mocked(getAcpSession).mock.calls.length).toBeGreaterThanOrEqual(2);
      expect(container.textContent).toContain('会话已经就绪');
    } finally {
      await unmount(root);
    }
  });

  it('replays the latest background text and tool event over a stale snapshot on the first entry', async () => {
    const prompt = event('prompt-1', 1, 'userTextDelta', '检查项目', {
      raw: { source: 'goldBandPrompt', promptId: 'prompt-1' },
    });
    const staleText = event('answer-1', 2, 'textDelta', '检查');
    const stale = session([prompt, staleText]);
    vi.mocked(getAcpSession).mockResolvedValue(stale);

    applyConversationEventToBranchSnapshots(update(event(
      'answer-1',
      3,
      'textDelta',
      '检查已经完成，准备调用工具',
      { startedSeq: 2 },
    )));
    applyConversationEventToBranchSnapshots(update(event(
      'tool-1',
      4,
      'toolCall',
      null,
      { title: 'Editing files', toolCallId: 'tool-1', status: 'running' },
    )));

    const { container, root } = await renderDialog(stale);
    try {
      expect(container.textContent).toContain('检查已经完成，准备调用工具');
      expect(container.textContent).toContain('Editing · files');
      expect([...container.querySelectorAll('[data-testid="markdown"]')]
        .every((node) => node.getAttribute('data-streaming') === 'false')).toBe(true);
    } finally {
      await unmount(root);
    }
  });

  it('reconciles a watermark-only gap with one revision delta query', async () => {
    const prompt = event('prompt-gap', 1, 'userTextDelta', '检查项目', {
      raw: { source: 'goldBandPrompt', promptId: 'prompt-gap' },
    });
    const stale = session([prompt, event('answer-gap', 2, 'textDelta', '延迟')]);
    const complete = session([
      prompt,
      event('answer-gap', 9, 'textDelta', '延迟追平后的完整回答', { startedSeq: 2 }),
    ]);
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(stale)
      .mockResolvedValue(complete);

    applyConversationEventToBranchSnapshots(update(event(
      'answer-gap',
      9,
      'textDelta',
      '延迟追平后的完整回答',
      {
        startedSeq: 2,
        raw: { oversized: 'x'.repeat(CONVERSATION_EVENT_REPLAY_LIMITS.eventBytes) },
      },
    )));

    const { container, root } = await renderDialog(stale);
    try {
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
      expect(vi.mocked(getAcpSession).mock.calls[1]?.[6]).toMatchObject({
        afterRevision: 2,
      });
      expect(container.textContent).toContain('延迟追平后的完整回答');
      expect([...container.querySelectorAll('[data-testid="markdown"]')]
        .every((node) => node.getAttribute('data-streaming') === 'false')).toBe(true);
    } finally {
      await unmount(root);
    }
  });

  it('keeps the animation gate closed and retries a failed replay delta', async () => {
    const prompt = event('prompt-retry-gap', 1, 'userTextDelta', '检查项目', {
      raw: { source: 'goldBandPrompt', promptId: 'prompt-retry-gap' },
    });
    const stale = session([prompt, event('answer-retry-gap', 2, 'textDelta', '延迟')]);
    const complete = session([
      prompt,
      event('answer-retry-gap', 9, 'textDelta', '重试后补齐的回答', { startedSeq: 2 }),
    ]);
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(stale)
      .mockRejectedValueOnce(new Error('temporary replay read failure'))
      .mockResolvedValue(complete);

    applyConversationEventToBranchSnapshots(update(event(
      'answer-retry-gap',
      9,
      'textDelta',
      '重试后补齐的回答',
      {
        startedSeq: 2,
        raw: { oversized: 'x'.repeat(CONVERSATION_EVENT_REPLAY_LIMITS.eventBytes) },
      },
    )));

    const { container, root } = await renderDialog(stale);
    try {
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 40));
      });
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
      expect([...container.querySelectorAll('[data-testid="markdown"]')]
        .every((node) => node.getAttribute('data-streaming') === 'false')).toBe(true);

      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 140));
      });
      expect(vi.mocked(getAcpSession).mock.calls.length).toBeGreaterThanOrEqual(3);
      expect(container.textContent).toContain('重试后补齐的回答');
      expect([...container.querySelectorAll('[data-testid="markdown"]')]
        .every((node) => node.getAttribute('data-streaming') === 'false')).toBe(true);
    } finally {
      await unmount(root);
    }
  });

  it('accepts a newer compacted snapshot without repeatedly refreshing an older loss generation', async () => {
    const compacted = session([
      event('prompt-compacted', 1, 'userTextDelta', '检查压缩后的会话'),
      event('answer-compacted', 9, 'textDelta', '当前页已经覆盖缺口'),
    ]);
    compacted.eventPage.generation = 2;
    compacted.eventPage.coveredRevision = 9;
    vi.mocked(getAcpSession).mockResolvedValue(compacted);

    applyConversationEventToBranchSnapshots(update(event(
      'answer-before-compaction',
      4,
      'textDelta',
      '旧 generation 的大事件',
      { raw: { oversized: 'x'.repeat(CONVERSATION_EVENT_REPLAY_LIMITS.eventBytes) } },
    )));

    const { root } = await renderDialog(compacted);
    try {
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(1);
    } finally {
      await unmount(root);
    }
  });

  it('does not query Agent branch content for a lifecycle-only notification', async () => {
    const branch = { ...session([]), branchId: 'agent-a' };
    vi.mocked(getAcpSession).mockResolvedValue(branch);

    const { root } = await renderDialog(branch, 'agent-a');
    try {
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(1);
      await act(async () => {
        runtime.listener?.({ ...locator, branchId: null });
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(1);
    } finally {
      await unmount(root);
    }
  });

  it('keeps history static and animates only a new post-handshake live delta', async () => {
    const historicalPrompt = event('prompt-old', 1, 'userTextDelta', '旧问题', {
      raw: { source: 'goldBandPrompt', promptId: 'prompt-old' },
    });
    const historicalAnswer = event('answer-old', 2, 'textDelta', '已经显示过的完整回答');
    const currentPrompt = event('prompt-new', 10, 'userTextDelta', '继续', {
      raw: { source: 'goldBandPrompt', promptId: 'prompt-new' },
    });
    const current = session([historicalPrompt, historicalAnswer, currentPrompt]);
    vi.mocked(getAcpSession).mockResolvedValue(current);
    let resolveHandshake: (() => void) | null = null;
    const handshake = new Promise<void>((resolve) => {
      resolveHandshake = resolve;
    });

    const { container, root } = await renderDialog(
      current,
      'root',
      (state) => {
        if (state === 'success') resolveHandshake?.();
      },
    );
    try {
      await act(async () => handshake);
      const historical = [...container.querySelectorAll('[data-testid="markdown"]')]
        .find((node) => node.textContent === '已经显示过的完整回答');
      expect(historical?.getAttribute('data-streaming')).toBe('false');

      await act(async () => {
        runtime.listener?.(update(event('answer-new', 11, 'textDelta', '新的实时回答')));
        await new Promise((resolve) => window.setTimeout(resolve, 150));
      });
      const live = [...container.querySelectorAll('[data-testid="markdown"]')]
        .find((node) => node.textContent === '新的实时回答');
      expect(live?.getAttribute('data-streaming')).toBe('true');

      await act(async () => {
        runtime.listener?.(update(event('tool-new', 12, 'toolCall', null, {
          title: 'Read file',
          toolCallId: 'tool-new',
          status: 'running',
        })));
        await new Promise((resolve) => window.setTimeout(resolve, 150));
      });
      expect(live?.getAttribute('data-streaming')).toBe('false');
    } finally {
      await unmount(root);
    }
  });
});
