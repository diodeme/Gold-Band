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
import {
  ACPChatDialog,
  loadedEventBufferLimit,
  optimisticUserEvent,
  resetAcpResourceCache,
} from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import {
  applyConversationEventToBranchSnapshots,
  CONVERSATION_EVENT_REPLAY_LIMITS,
  readConversationBranchReplaySnapshot,
  resetConversationEventRouterSnapshots,
} from '@/lib/conversation-event-router';
import {
  DEFAULT_ACP_CHAT_LOADED_EVENT_BUFFER_LIMIT,
} from '@/lib/acp-chat-pagination';
import type { AcpSessionUpdatedEventVm } from '@/api/client';
import type {
  AcpSessionVm,
  AcpUiEventVm,
  ConversationAttemptLifecycleVm,
} from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const locator = {
  projectId: 'project-watermark',
  taskId: 'task-watermark',
  taskUuid: 'task-uuid-watermark',
  runId: 'run-watermark',
  roundId: 'round-watermark',
  nodeId: 'node-watermark',
  attemptId: 'attempt-watermark',
};

type TestLocator = typeof locator & {
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
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
    pendingInteractions: [],
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
  optimisticEvents?: AcpUiEventVm[],
  dialogLocator: TestLocator = locator,
  eventPageSize?: number,
) {
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <TooltipProvider>
        <ACPChatDialog
          session={acpSession}
          {...dialogLocator}
          branchId={branchId}
          optimisticEvents={optimisticEvents}
          eventPageSize={eventPageSize}
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

function terminalLifecycle(turnId: string): ConversationAttemptLifecycleVm {
  return {
    runtime: {
      status: 'paused',
      outcome: null,
      pauseReason: 'user-paused',
      resumable: true,
      current: true,
      active: false,
      continuable: true,
      phase: 'idle',
      revision: 7,
    },
    control: { mode: 'non-runtime-controlled' },
    acp: {
      revision: 11,
      turnId,
      sessionAvailability: 'established',
      liveTurnActivity: 'idle',
      latestTurnStatus: 'completed',
      stopping: false,
    },
    displayStatus: 'paused',
    runtimeDisplay: {
      code: 'paused',
      tone: 'warning',
      icon: 'pause',
      terminal: false,
      resumable: true,
      reasonCode: 'user-paused',
      blockingError: false,
    },
    composer: {
      mode: 'normal',
      submitTarget: 'acp-prompt',
      processingKind: 'responding',
      canStop: false,
      lockInput: false,
    },
  };
}

function dynamicTerminalLifecycle(revision = 8): ConversationAttemptLifecycleVm {
  return {
    ...terminalLifecycle('turn-dynamic-completed'),
    runtime: {
      status: 'completed',
      outcome: 'success',
      pauseReason: null,
      resumable: false,
      current: false,
      active: false,
      continuable: false,
      phase: 'terminal',
      revision,
    },
    displayStatus: 'completed',
    runtimeDisplay: {
      code: 'completed',
      tone: 'success',
      icon: 'check',
      terminal: true,
      resumable: false,
      reasonCode: null,
      blockingError: false,
    },
  };
}

async function unmount(root: Root) {
  await act(async () => root.unmount());
}

beforeEach(() => {
  resetAcpResourceCache();
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
  resetAcpResourceCache();
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe('ACP session re-entry reconciliation', () => {
  it('refreshes dynamic terminal control output once and coalesces duplicate lifecycle-only notifications', async () => {
    const dynamicLocator: TestLocator = {
      ...locator,
      outerNodeId: 'ai-dynamic',
      outerAttemptId: 'attempt-001',
    };
    const json = '{"version":"0.1","kind":"dynamic-node-completion","status":"success"}';
    const stale = session([event('dynamic-result', 2, 'textDelta', json)], 'completed');
    const annotated = session([
      event('dynamic-result', 2, 'textDelta', json, {
        raw: {
          runtimeControlOutputDisplay: {
            kind: 'dynamic-node-completion',
            artifactName: 'dynamic-node-completion',
            jsonText: json,
            start: 0,
            end: json.length,
            parseStatus: 'valid',
          },
        },
      }),
    ], 'completed');
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(stale)
      .mockResolvedValueOnce(annotated);

    const { container, root } = await renderDialog(
      stale,
      'root',
      undefined,
      undefined,
      dynamicLocator,
    );
    try {
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(1);
      expect(container.querySelector('[data-theme-role="runtime-control"]')).toBeNull();

      await act(async () => {
        const update = {
          ...dynamicLocator,
          lifecycle: dynamicTerminalLifecycle(),
        };
        runtime.listener?.(update);
        runtime.listener?.(update);
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
      expect(container.querySelector('[data-theme-role="runtime-control"]')).not.toBeNull();
    } finally {
      await unmount(root);
    }
  });

  it('does not add a terminal content query for direct or normal workflow attempts', async () => {
    const completed = session([event('direct-result', 2, 'textDelta', 'done')], 'completed');
    vi.mocked(getAcpSession).mockResolvedValue(completed);

    const { root } = await renderDialog(completed);
    try {
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(1);
      await act(async () => {
        runtime.listener?.({
          ...locator,
          lifecycle: dynamicTerminalLifecycle(),
        });
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(1);
    } finally {
      await unmount(root);
    }
  });

  it('ignores a dynamic terminal refresh response after the selected leaf changes', async () => {
    const firstLocator: TestLocator = {
      ...locator,
      nodeId: 'goodbye-worker',
      outerNodeId: 'ai-dynamic',
      outerAttemptId: 'attempt-001',
    };
    const secondLocator: TestLocator = {
      ...firstLocator,
      nodeId: 'hello-worker',
    };
    const json = '{"kind":"dynamic-node-completion","status":"success"}';
    const stale = {
      ...session([event('old-result', 2, 'textDelta', json)], 'completed'),
      nodeId: firstLocator.nodeId,
    };
    const annotated = {
      ...stale,
      events: [event('old-result', 2, 'textDelta', json, {
        raw: {
          runtimeControlOutputDisplay: {
            kind: 'dynamic-node-completion',
            artifactName: 'dynamic-node-completion',
            jsonText: json,
            start: 0,
            end: json.length,
            parseStatus: 'valid',
          },
        },
      })],
    };
    const next = {
      ...session([event('new-result', 2, 'textDelta', 'hello worker selected')]),
      nodeId: secondLocator.nodeId,
    };
    let resolveOldRefresh: ((value: AcpSessionVm) => void) | null = null;
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(stale)
      .mockImplementationOnce(() => new Promise((resolve) => {
        resolveOldRefresh = resolve;
      }))
      .mockResolvedValue(next);

    const { container, root } = await renderDialog(
      stale,
      'root',
      undefined,
      undefined,
      firstLocator,
    );
    try {
      await act(async () => {
        runtime.listener?.({
          ...firstLocator,
          lifecycle: dynamicTerminalLifecycle(),
        });
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);

      await act(async () => {
        root.render(
          <TooltipProvider>
            <ACPChatDialog
              session={next}
              {...secondLocator}
              branchId="root"
              showSystemPromptAction={false}
              showRawFramesAction={false}
              usageCompact
            />
          </TooltipProvider>,
        );
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      await act(async () => {
        resolveOldRefresh?.(annotated);
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      expect(container.textContent).toContain('hello worker selected');
      expect(container.querySelector('[data-theme-role="runtime-control"]')).toBeNull();
    } finally {
      await unmount(root);
    }
  });

  it('settles stale optimistic response state from a terminal lifecycle received while unmounted', async () => {
    const turnId = 'turn-background-completed';
    const stale = session([
      event('prompt-background', 1, 'userTextDelta', '停止后追问', {
        raw: { source: 'goldBandPrompt', promptId: turnId },
      }),
      event('answer-background', 2, 'textDelta', '后台已经回复完成'),
    ], 'running');
    const optimistic = optimisticUserEvent('停止后追问', turnId, [], 0);
    vi.mocked(getAcpSession).mockResolvedValue(stale);
    applyConversationEventToBranchSnapshots({
      ...locator,
      lifecycle: terminalLifecycle(turnId),
    });

    const { container, root } = await renderDialog(
      stale,
      'root',
      undefined,
      [optimistic],
    );
    try {
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      expect(container.textContent).toContain('后台已经回复完成');
      expect(container.textContent).not.toContain('回复生成中');
      expect(container.querySelector<HTMLTextAreaElement>('textarea')?.disabled).toBe(false);
    } finally {
      await unmount(root);
    }
  });

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
    let rejectReplay!: (reason?: unknown) => void;
    const pendingReplay = new Promise<never>((_, reject) => {
      rejectReplay = reject;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(stale)
      .mockReturnValueOnce(pendingReplay)
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
      await vi.waitFor(() => {
        expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
      });
      expect([...container.querySelectorAll('[data-testid="markdown"]')]
        .every((node) => node.getAttribute('data-streaming') === 'false')).toBe(true);

      await act(async () => {
        rejectReplay(new Error('temporary replay read failure'));
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

  it('keeps older pagination reachable after live updates trim a full event window', async () => {
    const oldestPrompt = event('prompt-live-window', 1, 'userTextDelta', '窗口中最早的消息', {
      raw: { source: 'goldBandPrompt', promptId: 'prompt-live-window' },
    });
    const initial = session([oldestPrompt]);
    vi.mocked(getAcpSession).mockResolvedValue(initial);

    const { container, root } = await renderDialog(initial);
    try {
      await act(async () => {
        for (let index = 0; index < DEFAULT_ACP_CHAT_LOADED_EVENT_BUFFER_LIMIT; index += 1) {
          runtime.listener?.(update(event(
            `live-window-${index + 1}`,
            index + 2,
            'textDelta',
            `实时窗口消息 ${index + 1}`,
          )));
        }
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });

      expect(container.textContent).not.toContain('窗口中最早的消息');
      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => (
          element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto')
        ));
      expect(scroller).toBeDefined();
      Object.defineProperties(scroller!, {
        clientHeight: { configurable: true, value: 600 },
        scrollHeight: { configurable: true, value: 2_400 },
        scrollTop: { configurable: true, value: 0, writable: true },
      });

      await act(async () => {
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 20));
      });

      expect(vi.mocked(getAcpSession).mock.calls.some(
        (call) => typeof call[6]?.beforeSeq === 'number',
      )).toBe(true);
    } finally {
      await unmount(root);
    }
  });

  it('keeps a live update reachable when a stale newer-page response arrives late', async () => {
    const pageSize = 30;
    const loadedWindowSize = loadedEventBufferLimit(pageSize);
    const totalEventCount = loadedWindowSize;
    const currentWindowStart = pageSize + 1;
    const currentEvents = Array.from({ length: pageSize }, (_, index) => {
      const seq = currentWindowStart + index;
      return event(`current-window-${seq}`, seq, 'textDelta', `当前窗口消息 ${seq}`);
    });
    const initial = session(currentEvents);
    Object.assign(initial.eventPage, {
      coveredRevision: totalEventCount,
      newestRevision: currentEvents.at(-1)?.seq,
      total: totalEventCount,
      hasOlder: true,
      hasNewer: true,
    });

    const newerWindowStart = currentWindowStart + pageSize;
    const newerEvents = Array.from({ length: pageSize }, (_, index) => {
      const seq = newerWindowStart + index;
      return event(`newer-window-${seq}`, seq, 'textDelta', `下一窗口消息 ${seq}`);
    });
    const staleNewerPage = session(newerEvents);
    Object.assign(staleNewerPage.eventPage, {
      coveredRevision: totalEventCount,
      newestRevision: totalEventCount,
      total: totalEventCount,
      hasOlder: true,
      hasNewer: false,
    });

    let resolveNewerPage!: (value: AcpSessionVm) => void;
    const pendingNewerPage = new Promise<AcpSessionVm>((resolve) => {
      resolveNewerPage = resolve;
    });
    vi.mocked(getAcpSession).mockImplementation(async (...args) => {
      if (typeof args[6]?.afterSeq === 'number') return pendingNewerPage;
      return initial;
    });

    const { container, root } = await renderDialog(
      initial,
      'root',
      undefined,
      undefined,
      locator,
      pageSize,
    );
    try {
      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => (
          element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto')
        ));
      expect(scroller).toBeDefined();
      Object.defineProperties(scroller!, {
        clientHeight: { configurable: true, value: 600 },
        scrollHeight: { configurable: true, value: 2_400 },
        scrollTop: { configurable: true, value: 1_800, writable: true },
      });

      await act(async () => {
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 30));
      });
      expect(vi.mocked(getAcpSession).mock.calls.some(
        (call) => typeof call[6]?.afterSeq === 'number',
      )).toBe(true);

      const anchorText = `当前窗口消息 ${currentWindowStart}`;
      const findAnchor = () => [...container.querySelectorAll<HTMLElement>('[data-acp-item-key]')]
        .find((element) => element.textContent?.includes(anchorText));
      expect(findAnchor()).toBeDefined();
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).not.toBeNull();
      scroller!.scrollTop = 500;

      await act(async () => {
        runtime.listener?.(update(event(
          'live-during-newer-page-request',
          totalEventCount + 1,
          'textDelta',
          '迟到分页期间到达的实时消息',
        )));
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });
      expect(readConversationBranchReplaySnapshot(locator, 'root').events)
        .toEqual([expect.objectContaining({ id: 'live-during-newer-page-request' })]);
      expect(container.textContent).not.toContain('迟到分页期间到达的实时消息');

      await act(async () => {
        resolveNewerPage(staleNewerPage);
        await new Promise((resolve) => window.setTimeout(resolve, 30));
      });

      expect({
        anchorPresent: Boolean(findAnchor()),
        returnToLatestVisible: Boolean(
          container.querySelector('[data-acp-return-to-latest="true"]'),
        ),
        stalePageApplied: container.textContent?.includes(`下一窗口消息 ${newerWindowStart}`),
        liveEventInjected: container.textContent?.includes('迟到分页期间到达的实时消息'),
        scrollTop: scroller!.scrollTop,
      }).toEqual({
        anchorPresent: true,
        returnToLatestVisible: true,
        stalePageApplied: true,
        liveEventInjected: false,
        scrollTop: 500,
      });
    } finally {
      await unmount(root);
    }
  });

  it('keeps the historical window anchored and newer pagination reachable when live events arrive', async () => {
    const pageSize = 30;
    const loadedWindowSize = loadedEventBufferLimit(pageSize);
    const totalEventCount = loadedWindowSize * 2;
    const currentWindowStart = loadedWindowSize + 1;
    const currentEvents = Array.from({ length: loadedWindowSize }, (_, index) => {
      const seq = currentWindowStart + index;
      return event(`current-window-${seq}`, seq, 'textDelta', `当前窗口消息 ${seq}`);
    });
    const initial = session(currentEvents);
    Object.assign(initial.eventPage, {
      coveredRevision: totalEventCount,
      newestRevision: totalEventCount,
      total: totalEventCount,
      hasOlder: true,
    });

    const olderWindowStart = currentWindowStart - pageSize;
    const olderEvents = Array.from({ length: pageSize }, (_, index) => {
      const seq = olderWindowStart + index;
      return event(`older-window-${seq}`, seq, 'textDelta', `旧窗口锚点消息 ${seq}`);
    });
    const older = session(olderEvents);
    Object.assign(older.eventPage, {
      coveredRevision: totalEventCount,
      newestRevision: totalEventCount,
      total: totalEventCount,
      hasOlder: true,
      hasNewer: true,
    });

    vi.mocked(getAcpSession).mockImplementation(async (...args) => (
      typeof args[6]?.beforeSeq === 'number' ? older : initial
    ));

    const { container, root } = await renderDialog(
      initial,
      'root',
      undefined,
      undefined,
      locator,
      pageSize,
    );
    try {
      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => (
          element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto')
        ));
      expect(scroller).toBeDefined();
      Object.defineProperties(scroller!, {
        clientHeight: { configurable: true, value: 600 },
        scrollHeight: { configurable: true, value: 2_400 },
        scrollTop: { configurable: true, value: 0, writable: true },
      });

      await act(async () => {
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 30));
      });

      const anchorText = `旧窗口锚点消息 ${olderWindowStart}`;
      const findAnchor = () => [...container.querySelectorAll<HTMLElement>('[data-acp-item-key]')]
        .find((element) => element.textContent?.includes(anchorText));
      expect(findAnchor()).toBeDefined();
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).not.toBeNull();
      scroller!.scrollTop = 500;

      await act(async () => {
        runtime.listener?.(update(event(
          'live-beyond-historical-window',
          totalEventCount + 1,
          'textDelta',
          '历史阅读期间到达的实时消息',
        )));
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });

      expect({
        anchorPresent: Boolean(findAnchor()),
        returnToLatestVisible: Boolean(
          container.querySelector('[data-acp-return-to-latest="true"]'),
        ),
        scrollTop: scroller!.scrollTop,
      }).toEqual({
        anchorPresent: true,
        returnToLatestVisible: true,
        scrollTop: 500,
      });

      scroller!.scrollTop = scroller!.scrollHeight - scroller!.clientHeight;
      await act(async () => {
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 30));
      });

      expect(vi.mocked(getAcpSession).mock.calls.some(
        (call) => typeof call[6]?.afterSeq === 'number',
      )).toBe(true);
    } finally {
      await unmount(root);
    }
  });
});
