/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const runtime = vi.hoisted(() => ({
  listener: null as ((event: unknown) => void) | null,
}));

const streamingDiagnostics = vi.hoisted(() => ({
  enabled: false,
  records: [] as Array<{
    stage: string;
    details: Record<string, unknown>;
  }>,
}));

vi.mock('@/lib/acp-streaming-diagnostics', async () => {
  const actual = await vi.importActual<typeof import('@/lib/acp-streaming-diagnostics')>(
    '@/lib/acp-streaming-diagnostics',
  );
  return {
    ...actual,
    isAcpStreamingDiagnosticsEnabled: () => streamingDiagnostics.enabled,
    recordAcpStreamingDiagnostic: (
      stage: string,
      createDetails: () => Record<string, unknown>,
    ) => {
      if (stage !== 'return-to-latest-trace' && !streamingDiagnostics.enabled) return;
      streamingDiagnostics.records.push({ stage, details: createDetails() });
    },
  };
});

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
  return {
    ...actual,
    getAcpActivityDetail: vi.fn(),
    getAcpSession: vi.fn(),
    submitConversationPrompt: vi.fn(),
  };
});

vi.mock('@/components/prompt-kit/markdown', () => ({
  Markdown: ({ children, streaming }: { children: React.ReactNode; streaming?: boolean }) => (
    <div data-testid="markdown" data-streaming={streaming ? 'true' : 'false'}>{children}</div>
  ),
}));

import { getAcpActivityDetail, getAcpSession, submitConversationPrompt } from '@/api';
import {
  ACPChatDialog,
  createAcpEventWindowCacheKey,
  createAcpSessionCacheKey,
  loadedEventBufferLimit,
  optimisticUserEvent,
  resetAcpResourceCache,
  restoreAcpSession,
  updateAcpOptimisticEvents,
} from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import {
  applyConversationEventToBranchSnapshots,
  CONVERSATION_EVENT_REPLAY_LIMITS,
  readConversationBranchReplaySnapshot,
  resetConversationEventRouterSnapshots,
} from '@/lib/conversation-event-router';
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
  lifecycle?: ConversationAttemptLifecycleVm,
  allowEventOnlySessionShell = true,
) {
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  if (optimisticEvents) {
    const optimisticKey = createAcpSessionCacheKey(
      undefined,
      dialogLocator.taskId,
      dialogLocator.runId,
      dialogLocator.roundId,
      dialogLocator.nodeId,
      dialogLocator.attemptId,
      dialogLocator.projectId,
      dialogLocator.outerNodeId,
      dialogLocator.outerAttemptId,
      branchId,
    );
    updateAcpOptimisticEvents(optimisticKey, () => optimisticEvents);
  }
  await act(async () => {
    root.render(
      <TooltipProvider>
        <ACPChatDialog
          session={acpSession}
          {...dialogLocator}
          branchId={branchId}
          eventPageSize={eventPageSize}
          runtimeComposerContext={lifecycle ? {
            isOrchestrated: true,
            runtimeStatus: lifecycle.runtime.status,
            workflowValid: true,
            lifecycle,
          } : undefined}
          allowEventOnlySessionShell={allowEventOnlySessionShell}
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

async function renderStoredOptimisticDialog(
  acpSession: AcpSessionVm,
  initialOptimisticEvents: AcpUiEventVm[],
  eventPageSize?: number,
) {
  const optimisticKey = createAcpSessionCacheKey(
    undefined,
    locator.taskId,
    locator.runId,
    locator.roundId,
    locator.nodeId,
    locator.attemptId,
    locator.projectId,
    undefined,
    undefined,
    'root',
  );
  updateAcpOptimisticEvents(optimisticKey, () => initialOptimisticEvents);
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <TooltipProvider>
        <ACPChatDialog
          session={acpSession}
          {...locator}
          branchId="root"
          eventPageSize={eventPageSize}
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

async function detachConversationViewport(container: HTMLElement) {
  const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
    .find((element) => element.classList.contains('h-full')
      && element.classList.contains('overflow-y-auto'));
  expect(scroller).toBeDefined();
  if (scroller!.scrollHeight <= scroller!.clientHeight) {
    Object.defineProperties(scroller!, {
      clientHeight: { configurable: true, value: 100 },
      scrollHeight: { configurable: true, value: 1_000 },
      scrollTop: { configurable: true, value: 500, writable: true },
    });
  } else if (
    scroller!.scrollHeight - scroller!.scrollTop - scroller!.clientHeight <= 2
  ) {
    scroller!.scrollTop = Math.max(
      0,
      scroller!.scrollHeight - scroller!.clientHeight - 100,
    );
  }
  await act(async () => {
    scroller!.dispatchEvent(new WheelEvent('wheel', { bubbles: true, deltaY: -1 }));
    await new Promise((resolve) => window.setTimeout(resolve, 0));
  });
  return scroller!;
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

function activePermissionLifecycle(turnId: string): ConversationAttemptLifecycleVm {
  const lifecycle = terminalLifecycle(turnId);
  lifecycle.runtime = {
    status: 'running',
    outcome: null,
    pauseReason: null,
    resumable: false,
    current: true,
    active: true,
    continuable: false,
    phase: 'provider-running',
    revision: 1,
  };
  lifecycle.acp = {
    revision: 5,
    turnId,
    sessionAvailability: 'established',
    liveTurnActivity: 'waiting-permission',
    latestTurnStatus: 'running',
    stopping: false,
  };
  lifecycle.composer = {
    mode: 'runtime-active',
    submitTarget: 'none',
    processingKind: 'processing',
    canStop: true,
    lockInput: true,
  };
  return lifecycle;
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

async function setTextareaValue(textarea: HTMLTextAreaElement, value: string) {
  const valueSetter = Object.getOwnPropertyDescriptor(
    HTMLTextAreaElement.prototype,
    'value',
  )?.set;
  await act(async () => {
    valueSetter?.call(textarea, value);
    textarea.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

beforeEach(() => {
  streamingDiagnostics.enabled = false;
  streamingDiagnostics.records = [];
  resetAcpResourceCache();
  resetConversationEventRouterSnapshots();
  vi.mocked(getAcpActivityDetail).mockReset();
  vi.mocked(getAcpSession).mockReset();
  vi.mocked(submitConversationPrompt).mockReset();
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

  it('hydrates a durable agent reply while the selected dynamic session stays mounted', async () => {
    const dynamicLocator: TestLocator = {
      ...locator,
      outerNodeId: 'ai-dynamic',
      outerAttemptId: 'attempt-001',
    };
    const prompt = event('prompt-mounted', 1, 'userTextDelta', '执行自动任务', {
      raw: { source: 'goldBandPrompt', promptId: 'prompt-mounted' },
    });
    const stale = session([prompt], 'running');
    const completed = session([
      prompt,
      event('answer-mounted', 2, 'textDelta', 'Agent 已经完成自动任务'),
    ], 'completed');
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(stale)
      .mockResolvedValueOnce(completed);

    const { container, root } = await renderDialog(
      stale,
      'root',
      undefined,
      undefined,
      dynamicLocator,
    );
    try {
      expect(container.textContent).toContain('执行自动任务');
      expect(container.textContent).not.toContain('Agent 已经完成自动任务');

      await act(async () => {
        runtime.listener?.({
          ...dynamicLocator,
          lifecycle: dynamicTerminalLifecycle(),
        });
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
      expect(container.textContent).toContain('Agent 已经完成自动任务');
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

  it('does not let a cached transitional runtime error replace the converged canonical composer', async () => {
    const turnId = 'turn-auth-unavailable';
    const transitional: ConversationAttemptLifecycleVm = {
      ...terminalLifecycle(turnId),
      runtime: {
        status: 'running',
        outcome: null,
        pauseReason: null,
        resumable: false,
        current: true,
        active: false,
        continuable: false,
        phase: 'idle',
        revision: 3,
      },
      acp: {
        revision: 11,
        turnId,
        sessionAvailability: 'restorable',
        liveTurnActivity: 'idle',
        latestTurnStatus: 'failed',
        stopping: false,
      },
      displayStatus: 'failed',
      runtimeDisplay: {
        code: 'failure',
        tone: 'danger',
        icon: 'error',
        terminal: true,
        resumable: false,
        reasonCode: null,
        blockingError: true,
      },
      continueKind: null,
      composer: {
        mode: 'runtime-error',
        submitTarget: 'none',
        processingKind: 'processing',
        canStop: false,
        lockInput: true,
      },
    };
    const canonical: ConversationAttemptLifecycleVm = {
      ...transitional,
      runtime: {
        status: 'paused',
        outcome: null,
        pauseReason: 'runtime-abnormal',
        resumable: true,
        current: true,
        active: false,
        continuable: false,
        phase: 'idle',
        revision: 4,
      },
      displayStatus: 'paused',
      runtimeDisplay: {
        code: 'runtime-abnormal',
        tone: 'danger',
        icon: 'error',
        terminal: false,
        resumable: true,
        reasonCode: 'runtime-abnormal',
        blockingError: false,
      },
      composer: {
        mode: 'normal',
        submitTarget: 'acp-prompt',
        processingKind: 'processing',
        canStop: false,
        lockInput: false,
      },
    };
    const failedSession = session([
      event('auth-failure', 2, 'error', 'Reconnecting... 5/5'),
    ], 'failed');
    vi.mocked(getAcpSession).mockResolvedValue(failedSession);
    applyConversationEventToBranchSnapshots({
      ...locator,
      lifecycle: transitional,
    });

    const { container, root } = await renderDialog(
      failedSession,
      'root',
      undefined,
      undefined,
      locator,
      undefined,
      canonical,
    );
    try {
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
      expect(textarea).not.toBeNull();
      expect(textarea?.disabled).toBe(false);
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

  it('keeps return-to-latest hidden while a new session has only a pending timeline', async () => {
    const placeholder = session([], 'pending');
    placeholder.sessionId = null;
    placeholder.eventPage.generation = 0;
    placeholder.eventPage.hasNewer = true;
    const lifecycle: ConversationAttemptLifecycleVm = {
      ...terminalLifecycle('turn-initializing'),
      runtime: {
        status: 'running',
        outcome: null,
        pauseReason: null,
        resumable: false,
        current: true,
        active: true,
        continuable: false,
        phase: 'provider-running',
        revision: 1,
      },
      acp: {
        revision: 1,
        turnId: 'turn-initializing',
        sessionAvailability: 'unavailable',
        liveTurnActivity: 'running',
        latestTurnStatus: 'none',
        stopping: false,
      },
      displayStatus: 'running',
      runtimeDisplay: {
        code: 'running',
        tone: 'running',
        icon: 'dot',
        terminal: false,
        resumable: false,
        reasonCode: null,
        blockingError: false,
      },
      composer: {
        mode: 'runtime-active',
        submitTarget: 'none',
        processingKind: 'processing',
        statusKey: 'conversation.runtime.runtimeActive',
        canStop: true,
        lockInput: true,
      },
    };
    vi.mocked(getAcpSession).mockImplementation(() => new Promise(() => {}));

    const { container, root } = await renderDialog(
      placeholder,
      'root',
      undefined,
      undefined,
      locator,
      undefined,
      lifecycle,
      false,
    );
    try {
      expect(container.querySelector('[data-brand-loading-state="true"]')).not.toBeNull();
      expect(container.querySelector('[data-acp-item-key]')).toBeNull();
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
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

  it('recovers the canonical body when a pending permission arrives over an empty visible timeline', async () => {
    const hiddenRuntimePrompt = event(
      'permission-gap-prompt',
      1,
      'userTextDelta',
      'runtime prompt',
      {
        raw: {
          source: 'goldBandPrompt',
          promptId: 'permission-gap-prompt',
          hiddenFromChat: true,
        },
      },
    );
    const permission = event(
      'permission-json-rpc-3',
      5,
      'permissionRequest',
      null,
      {
        title: 'Run project search',
        toolCallId: 'tool-permission-gap',
        status: 'pending',
        raw: {
          requestId: '3',
          options: [
            { optionId: 'allow_once', name: 'Allow once', kind: 'allow_once' },
            { optionId: 'reject_once', name: 'Reject', kind: 'reject_once' },
          ],
        },
      },
    );
    const stale = session([hiddenRuntimePrompt]);
    const canonical = session([
      hiddenRuntimePrompt,
      event('permission-gap-answer', 2, 'textDelta', '先前已经产生的回复'),
      event('permission-gap-tool', 3, 'toolCall', null, {
        title: 'Editing files',
        toolCallId: 'tool-permission-gap',
        status: 'running',
      }),
      permission,
    ]);
    for (const snapshot of [stale, canonical]) {
      snapshot.systemPromptAppend = 'runtime context';
      snapshot.config = {
        currentModelId: 'test-model',
        currentModeId: 'test-mode',
      };
    }
    const activeLifecycle = activePermissionLifecycle('turn-permission-gap');
    let canonicalAvailable = false;
    vi.mocked(getAcpSession).mockImplementation(async () => (
      canonicalAvailable ? canonical : stale
    ));

    const { container, root } = await renderDialog(
      stale,
      'root',
      undefined,
      undefined,
      locator,
      undefined,
      activeLifecycle,
    );
    try {
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 100));
      });
      const initialReadCount = vi.mocked(getAcpSession).mock.calls.length;
      expect(container.querySelector('[data-brand-loading-state="true"]')).not.toBeNull();

      canonicalAvailable = true;
      await act(async () => {
        runtime.listener?.({
          ...update(permission),
          timelineRevision: 5,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 100));
      });

      expect(container.textContent).toContain('Run project search');
      await vi.waitFor(() => {
        expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(initialReadCount + 1);
      });
      expect(container.textContent).toContain('先前已经产生的回复');
      expect(container.querySelector('[data-brand-loading-state="true"]')).toBeNull();
      expect(container.textContent).toContain('Run project search');

      await act(async () => {
        runtime.listener?.({
          ...update(permission),
          timelineRevision: 5,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 250));
      });
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(initialReadCount + 1);
    } finally {
      await unmount(root);
    }
  });

  it('refreshes stale visible body content when a pending permission advances the canonical revision', async () => {
    const prompt = event('stale-permission-prompt', 1, 'userTextDelta', '检查项目', {
      raw: { source: 'goldBandPrompt', promptId: 'stale-permission-prompt' },
    });
    const permission = event(
      'permission-json-rpc-4',
      5,
      'permissionRequest',
      null,
      {
        title: 'Approve project edit',
        toolCallId: 'tool-stale-permission',
        status: 'pending',
        raw: {
          requestId: '4',
          options: [
            { optionId: 'allow_once', name: 'Allow once', kind: 'allow_once' },
            { optionId: 'reject_once', name: 'Reject', kind: 'reject_once' },
          ],
        },
      },
    );
    const stale = session([
      prompt,
      event('stale-permission-answer', 2, 'textDelta', '仍停留在旧版本的回复'),
    ]);
    const canonical = session([
      prompt,
      event('stale-permission-answer', 4, 'textDelta', '后台已经产生的最新回复', {
        startedSeq: 2,
      }),
      permission,
    ]);
    let canonicalAvailable = false;
    vi.mocked(getAcpSession).mockImplementation(async () => (
      canonicalAvailable ? canonical : stale
    ));

    const { container, root } = await renderDialog(
      stale,
      'root',
      undefined,
      undefined,
      locator,
      undefined,
      activePermissionLifecycle('turn-stale-permission'),
    );
    try {
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 100));
      });
      const initialReadCount = vi.mocked(getAcpSession).mock.calls.length;
      expect(container.textContent).toContain('仍停留在旧版本的回复');

      canonicalAvailable = true;
      await act(async () => {
        runtime.listener?.({
          ...update(permission),
          timelineRevision: 5,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 100));
      });

      expect(container.textContent).toContain('Approve project edit');
      await vi.waitFor(() => {
        expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(initialReadCount + 1);
      });
      expect(container.textContent).toContain('后台已经产生的最新回复');
      expect(container.textContent).not.toContain('仍停留在旧版本的回复');
      expect(container.textContent).toContain('Approve project edit');
    } finally {
      await unmount(root);
    }
  });

  it('projects a pending canonical body over a stale window without manual history intent', async () => {
    const prompt = event('layout-permission-prompt', 1, 'userTextDelta', '检查布局恢复', {
      raw: { source: 'goldBandPrompt', promptId: 'layout-permission-prompt' },
    });
    const permission = event(
      'permission-json-rpc-layout',
      5,
      'permissionRequest',
      null,
      {
        title: 'Approve after app switch',
        toolCallId: 'tool-layout-permission',
        status: 'pending',
        raw: {
          requestId: 'layout',
          options: [
            { optionId: 'allow_once', name: 'Allow once', kind: 'allow_once' },
            { optionId: 'reject_once', name: 'Reject', kind: 'reject_once' },
          ],
        },
      },
    );
    const stale = session([
      prompt,
      event('layout-permission-answer', 2, 'textDelta', '切换应用前的旧回复'),
    ]);
    stale.eventPage.hasNewer = true;
    const canonical = session([
      prompt,
      event('layout-permission-answer', 4, 'textDelta', '切回应用后的 canonical 回复', {
        startedSeq: 2,
      }),
      permission,
    ]);
    vi.mocked(getAcpSession).mockResolvedValue(stale);

    const { container, root } = await renderDialog(
      stale,
      'root',
      undefined,
      undefined,
      locator,
      undefined,
      activePermissionLifecycle('turn-layout-permission'),
    );
    try {
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });

      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          timelineGeneration: 1,
          timelineRevision: 5,
          session: canonical,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 100));
      });

      expect(container.textContent).toContain('切回应用后的 canonical 回复');
      expect(container.textContent).not.toContain('切换应用前的旧回复');
      expect(container.textContent).toContain('Approve after app switch');
    } finally {
      await unmount(root);
    }
  });

  it('preserves a manually detached history window when a pending permission advances the revision', async () => {
    const prompt = event('manual-permission-prompt', 1, 'userTextDelta', '检查历史', {
      raw: { source: 'goldBandPrompt', promptId: 'manual-permission-prompt' },
    });
    const permission = event(
      'permission-json-rpc-5',
      5,
      'permissionRequest',
      null,
      {
        title: 'Approve while reading history',
        toolCallId: 'tool-manual-permission',
        status: 'pending',
        raw: {
          requestId: '5',
          options: [
            { optionId: 'allow_once', name: 'Allow once', kind: 'allow_once' },
            { optionId: 'reject_once', name: 'Reject', kind: 'reject_once' },
          ],
        },
      },
    );
    const stale = session([
      prompt,
      event('manual-permission-answer', 2, 'textDelta', '用户正在阅读的历史回复'),
    ]);
    const canonical = session([
      prompt,
      event('manual-permission-answer', 4, 'textDelta', '最新回复不应强制覆盖历史窗口', {
        startedSeq: 2,
      }),
      permission,
    ]);
    let canonicalAvailable = false;
    vi.mocked(getAcpSession).mockImplementation(async () => (
      canonicalAvailable ? canonical : stale
    ));

    const { container, root } = await renderDialog(
      stale,
      'root',
      undefined,
      undefined,
      locator,
      undefined,
      activePermissionLifecycle('turn-manual-permission'),
    );
    try {
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 100));
      });
      await detachConversationViewport(container);
      const initialReadCount = vi.mocked(getAcpSession).mock.calls.length;

      canonicalAvailable = true;
      await act(async () => {
        runtime.listener?.({
          ...update(permission),
          timelineRevision: 5,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 250));
      });

      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(initialReadCount);
      expect(container.textContent).toContain('用户正在阅读的历史回复');
      expect(container.textContent).not.toContain('最新回复不应强制覆盖历史窗口');
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).not.toBeNull();
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

  it('automatically rereads the canonical head until a sequence-only replay loss is covered', async () => {
    const stale = session([
      event('sequence-loss-answer', 2, 'textDelta', 'sequence loss 前的旧内容'),
    ]);
    const covered = session([
      event('sequence-loss-answer', 9, 'textDelta', 'sequence loss 已由 canonical head 覆盖', {
        startedSeq: 2,
        endedSeq: 9,
      }),
    ]);
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(stale)
      .mockResolvedValue(covered);

    applyConversationEventToBranchSnapshots({
      ...update(event(
        'sequence-loss-answer',
        9,
        'textDelta',
        '尚未落盘的 sequence-only live',
        {
          startedSeq: 2,
          endedSeq: 9,
          raw: { oversized: 'x'.repeat(CONVERSATION_EVENT_REPLAY_LIMITS.eventBytes) },
        },
      )),
      timelineRevision: null,
    });
    expect(readConversationBranchReplaySnapshot(locator, 'root')).toMatchObject({
      lossWatermarkRevision: 0,
      lossWatermarkSeq: 9,
      requiresCatchUp: true,
    });

    const { container, root } = await renderDialog(stale);
    try {
      await vi.waitFor(() => {
        expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
      });

      expect(vi.mocked(getAcpSession).mock.calls.map((call) => call[6]?.afterRevision))
        .toEqual([undefined, undefined]);
      expect(container.textContent).toContain('sequence loss 已由 canonical head 覆盖');
      expect(container.textContent).not.toContain('sequence loss 前的旧内容');
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
      expect(readConversationBranchReplaySnapshot(locator, 'root')).toMatchObject({
        lossWatermarkSeq: 0,
        requiresCatchUp: false,
      });
    } finally {
      await unmount(root);
    }
  });

  it('does not acknowledge sequence loss from a subscription-only session watermark during initial re-entry', async () => {
    const stale = session([
      event('subscription-watermark-answer', 2, 'textDelta', 'canonical head 仍未覆盖 sequence loss'),
    ]);
    const subscriptionOnly = session([
      event('subscription-watermark-answer', 20, 'textDelta', 'subscription 可展示最新内容', {
        startedSeq: 2,
        endedSeq: 20,
      }),
    ]);
    Object.assign(subscriptionOnly.eventPage, {
      coveredRevision: 2,
      newestRevision: 2,
      newestSeq: 20,
    });
    const covered = session([
      event('subscription-watermark-answer', 9, 'textDelta', 'canonical full-head 已覆盖 sequence loss', {
        startedSeq: 2,
        endedSeq: 9,
      }),
    ]);
    let resolveInitialRead!: (value: AcpSessionVm) => void;
    const initialRead = new Promise<AcpSessionVm>((resolve) => {
      resolveInitialRead = resolve;
    });
    let resolveCanonicalRead!: (value: AcpSessionVm) => void;
    const canonicalRead = new Promise<AcpSessionVm>((resolve) => {
      resolveCanonicalRead = resolve;
    });
    vi.mocked(getAcpSession)
      .mockReturnValueOnce(initialRead)
      .mockReturnValueOnce(canonicalRead)
      .mockResolvedValue(covered);

    applyConversationEventToBranchSnapshots({
      ...update(event(
        'subscription-watermark-answer',
        9,
        'textDelta',
        '尚未落盘的 sequence-only live',
        {
          startedSeq: 2,
          endedSeq: 9,
          raw: { oversized: 'x'.repeat(CONVERSATION_EVENT_REPLAY_LIMITS.eventBytes) },
        },
      )),
      timelineRevision: null,
    });

    const { container, root } = await renderDialog(stale);
    try {
      await vi.waitFor(() => {
        expect(runtime.listener).not.toBeNull();
      });
      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          timelineGeneration: 1,
          timelineRevision: 2,
          session: subscriptionOnly,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
      expect(container.textContent).toContain('canonical head 仍未覆盖 sequence loss');
      expect(container.textContent).not.toContain('subscription 可展示最新内容');

      await act(async () => {
        resolveInitialRead(stale);
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      await vi.waitFor(() => {
        expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
      });
      expect(readConversationBranchReplaySnapshot(locator, 'root')).toMatchObject({
        lossWatermarkSeq: 9,
        requiresCatchUp: true,
      });
      expect(container.textContent).toContain('canonical head 仍未覆盖 sequence loss');
      expect(container.textContent).not.toContain('subscription 可展示最新内容');

      await act(async () => {
        resolveCanonicalRead(covered);
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      await vi.waitFor(() => {
        expect(readConversationBranchReplaySnapshot(locator, 'root')).toMatchObject({
          lossWatermarkSeq: 0,
          requiresCatchUp: false,
        });
      });
      expect(container.textContent).toContain('canonical full-head 已覆盖 sequence loss');
      expect(container.textContent).not.toContain('subscription 可展示最新内容');
      expect(vi.mocked(getAcpSession).mock.calls.map((call) => call[6]?.afterRevision))
        .toEqual([undefined, undefined]);
    } finally {
      await unmount(root);
    }
  });

  it('does not acknowledge sequence loss from a replay-projected visible sequence', async () => {
    const historical = session([
      event('visible-sequence-history', 1, 'textDelta', '仍在阅读的历史窗口'),
    ]);
    Object.assign(historical.eventPage, {
      coveredRevision: 1,
      newestRevision: 1,
      total: 2,
      hasNewer: true,
    });
    const canonicalBeforeReplay = session([
      event('visible-sequence-canonical', 1, 'textDelta', 'replay 投影前的 canonical head'),
    ]);
    const canonicalStillBehindLoss = session([
      event('visible-sequence-canonical', 10, 'textDelta', '第一次 recovery 的 canonical 仍落后'),
    ]);
    const canonicalCoveredLoss = session([
      event('visible-sequence-loss', 15, 'textDelta', '真正覆盖 sequence loss 的 canonical head'),
    ]);
    let resolveCanonicalCoveredLoss!: (value: AcpSessionVm) => void;
    const pendingCanonicalCoveredLoss = new Promise<AcpSessionVm>((resolve) => {
      resolveCanonicalCoveredLoss = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(historical)
      .mockResolvedValueOnce(canonicalBeforeReplay)
      .mockResolvedValueOnce(canonicalStillBehindLoss)
      .mockReturnValue(pendingCanonicalCoveredLoss);

    applyConversationEventToBranchSnapshots({
      ...update(event(
        'visible-sequence-replay',
        20,
        'textDelta',
        '只来自 replay 的可见高 sequence',
      )),
      timelineRevision: null,
    });

    const { container, root } = await renderDialog(historical);
    try {
      await detachConversationViewport(container);
      const returnToLatest = container.querySelector<HTMLButtonElement>(
        '[data-acp-return-to-latest="true"]',
      );
      expect(returnToLatest).not.toBeNull();
      await act(async () => {
        returnToLatest!.click();
        await vi.waitFor(() => {
          expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
        });
      });
      await vi.waitFor(() => {
        expect(container.textContent).toContain('只来自 replay 的可见高 sequence');
      });
      const cacheKey = createAcpEventWindowCacheKey({
        ...locator,
        branchId: 'root',
      });
      expect(restoreAcpSession(cacheKey)?.eventPage.newestSeq).toBe(20);

      applyConversationEventToBranchSnapshots({
        ...update(event(
          'visible-sequence-loss',
          15,
          'textDelta',
          '尚未被 canonical 覆盖的 sequence loss',
          { raw: { oversized: 'x'.repeat(CONVERSATION_EVENT_REPLAY_LIMITS.eventBytes) } },
        )),
        timelineRevision: null,
      });
      expect(readConversationBranchReplaySnapshot(locator, 'root')).toMatchObject({
        lossWatermarkRevision: 0,
        lossWatermarkSeq: 15,
        requiresCatchUp: true,
      });

      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          event: event(
            'visible-sequence-malformed',
            21,
            'textDelta',
            '触发 canonical recovery 的非法 live',
          ),
          timelineRevision: null,
        });
      });
      await vi.waitFor(() => {
        expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(3);
      });
      expect(readConversationBranchReplaySnapshot(locator, 'root')).toMatchObject({
        lossWatermarkSeq: 15,
        requiresCatchUp: true,
      });
      await vi.waitFor(() => {
        expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(4);
      });
      await act(async () => {
        resolveCanonicalCoveredLoss(canonicalCoveredLoss);
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      expect(vi.mocked(getAcpSession).mock.calls.map((call) => call[6]?.afterRevision))
        .toEqual([undefined, undefined, undefined, undefined]);
      expect(container.textContent).toContain('真正覆盖 sequence loss 的 canonical head');
      expect(container.textContent).not.toContain('只来自 replay 的可见高 sequence');
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
      expect(readConversationBranchReplaySnapshot(locator, 'root')).toMatchObject({
        lossWatermarkSeq: 0,
        requiresCatchUp: false,
      });
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

  it('refreshes a stale snapshot before projecting retained replay from a newer generation', async () => {
    const prompt = event('prompt-generation-refresh', 1, 'userTextDelta', '检查新代际');
    const stale = session([
      prompt,
      event('answer-generation-stale', 2, 'textDelta', '旧代际回答'),
    ]);
    const refreshed = session([
      prompt,
      event('answer-generation-current', 8, 'textDelta', '新代际完整回答', {
        startedSeq: 2,
      }),
    ]);
    refreshed.eventPage.generation = 2;
    refreshed.eventPage.coveredRevision = 8;
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(stale)
      .mockResolvedValue(refreshed);
    applyConversationEventToBranchSnapshots({
      ...update(event(
        'answer-generation-current',
        8,
        'textDelta',
        '新代际完整回答',
        { startedSeq: 2 },
      )),
      timelineGeneration: 2,
      timelineRevision: 8,
    });

    const { container, root } = await renderDialog(stale);
    try {
      await vi.waitFor(() => {
        expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
      });
      expect(container.textContent).toContain('新代际完整回答');
      expect(container.textContent).not.toContain('旧代际回答');
    } finally {
      await unmount(root);
    }
  });

  it('does not project retained replay from an older generation over a newer snapshot', async () => {
    const compacted = session([
      event('prompt-generation-current', 1, 'userTextDelta', '检查当前代际'),
      event('answer-generation-current', 8, 'textDelta', '当前代际回答'),
    ]);
    compacted.eventPage.generation = 2;
    compacted.eventPage.coveredRevision = 8;
    vi.mocked(getAcpSession).mockResolvedValue(compacted);
    applyConversationEventToBranchSnapshots(update(event(
      'answer-generation-stale',
      7,
      'textDelta',
      '不应出现的旧代际回答',
    )));

    const { container, root } = await renderDialog(compacted);
    try {
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
      expect(container.textContent).toContain('当前代际回答');
      expect(container.textContent).not.toContain('不应出现的旧代际回答');
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(1);
    } finally {
      await unmount(root);
    }
  });

  it('resets the covered revision when the initial snapshot advances generation', async () => {
    const previousGeneration = session([
      event('answer-before-generation-reset', 100, 'textDelta', '旧代际高水位'),
    ]);
    previousGeneration.eventPage.coveredRevision = 100;
    previousGeneration.eventPage.newestRevision = 100;
    const compacted = session([
      event('answer-after-generation-reset', 5, 'textDelta', '新代际尚未追平'),
    ]);
    compacted.eventPage.generation = 2;
    compacted.eventPage.coveredRevision = 5;
    compacted.eventPage.newestRevision = 5;
    const caughtUp = session([
      event('answer-after-generation-reset', 9, 'textDelta', '新代际尚未追平，随后已经完成追平', {
        startedSeq: 5,
      }),
    ]);
    caughtUp.eventPage.generation = 2;
    caughtUp.eventPage.coveredRevision = 9;
    caughtUp.eventPage.newestRevision = 9;
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(compacted)
      .mockResolvedValue(caughtUp);
    applyConversationEventToBranchSnapshots({
      ...update(event(
        'answer-after-generation-reset',
        9,
        'textDelta',
        '新代际尚未追平，随后已经完成追平',
        {
          startedSeq: 5,
          raw: { oversized: 'x'.repeat(CONVERSATION_EVENT_REPLAY_LIMITS.eventBytes) },
        },
      )),
      timelineGeneration: 2,
      timelineRevision: 9,
    });

    const { container, root } = await renderDialog(previousGeneration);
    try {
      await vi.waitFor(() => {
        expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
        expect(container.textContent).toContain('随后已经完成追平');
      });
      expect(vi.mocked(getAcpSession).mock.calls[1]?.[6]).toMatchObject({
        afterRevision: 5,
      });
      expect(container.textContent).not.toContain('旧代际高水位');
    } finally {
      await unmount(root);
    }
  });

  it('keeps a newer subscription generation when an awaited replay refresh returns late', async () => {
    const stale = session([
      event('answer-generation-race-stale', 2, 'textDelta', '一代旧回答'),
    ]);
    const refreshGeneration = session([
      event('answer-generation-race-refresh', 8, 'textDelta', '二代迟到刷新'),
    ]);
    refreshGeneration.eventPage.generation = 2;
    refreshGeneration.eventPage.coveredRevision = 8;
    const subscriptionGeneration = session([
      event('answer-generation-race-current', 12, 'textDelta', '三代当前回答'),
    ]);
    subscriptionGeneration.eventPage.generation = 3;
    subscriptionGeneration.eventPage.coveredRevision = 12;
    let resolveRefresh!: (value: AcpSessionVm) => void;
    const pendingRefresh = new Promise<AcpSessionVm>((resolve) => {
      resolveRefresh = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(stale)
      .mockReturnValueOnce(pendingRefresh);
    applyConversationEventToBranchSnapshots({
      ...update(event(
        'answer-generation-race-replay',
        8,
        'textDelta',
        '二代 replay 不应混入',
      )),
      timelineGeneration: 2,
      timelineRevision: 8,
    });

    const { container, root } = await renderDialog(stale);
    try {
      await vi.waitFor(() => {
        expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
      });
      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          timelineGeneration: 3,
          timelineRevision: 12,
          session: subscriptionGeneration,
        });
        resolveRefresh(refreshGeneration);
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      await vi.waitFor(() => {
        expect(container.textContent).toContain('三代当前回答');
      });
      expect(container.textContent).not.toContain('一代旧回答');
      expect(container.textContent).not.toContain('二代迟到刷新');
      expect(container.textContent).not.toContain('二代 replay 不应混入');
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
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

  it('ignores a late Agent-branch refresh after the selected eventWindowKey changes', async () => {
    const firstLocator = {
      ...locator,
      nodeId: 'agent-owner-a',
      attemptId: 'agent-attempt-a',
    };
    const secondLocator = {
      ...locator,
      nodeId: 'agent-owner-b',
      attemptId: 'agent-attempt-b',
    };
    const first = {
      ...session([event('agent-a-initial', 1, 'textDelta', 'Agent A initial')]),
      branchId: 'agent-a',
      nodeId: firstLocator.nodeId,
      attemptId: firstLocator.attemptId,
    };
    const lateFirst = {
      ...session([event('agent-a-late', 2, 'textDelta', 'Agent A late refresh')]),
      branchId: 'agent-a',
      nodeId: firstLocator.nodeId,
      attemptId: firstLocator.attemptId,
    };
    const second = {
      ...session([event('agent-b-current', 1, 'textDelta', 'Agent B current')]),
      branchId: 'agent-b',
      nodeId: secondLocator.nodeId,
      attemptId: secondLocator.attemptId,
    };
    let resolveLateFirst!: (value: AcpSessionVm) => void;
    let firstLocatorRequestCount = 0;
    vi.mocked(getAcpSession).mockImplementation(async (...args) => {
      if (args[4] === secondLocator.nodeId) return second;
      firstLocatorRequestCount += 1;
      if (firstLocatorRequestCount === 1) return first;
      return new Promise<AcpSessionVm>((resolve) => {
        resolveLateFirst = resolve;
      });
    });

    const { container, root } = await renderDialog(
      first,
      'agent-a',
      undefined,
      undefined,
      firstLocator,
    );
    try {
      await act(async () => {
        runtime.listener?.({
          ...firstLocator,
          branchId: 'agent-a',
          session: first,
        });
        await vi.waitFor(() => expect(firstLocatorRequestCount).toBe(2));
      });

      await act(async () => {
        root.render(
          <TooltipProvider>
            <ACPChatDialog
              session={second}
              {...secondLocator}
              branchId="agent-b"
              showSystemPromptAction={false}
              showRawFramesAction={false}
              usageCompact
            />
          </TooltipProvider>,
        );
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
      expect(container.textContent).toContain('Agent B current');

      await act(async () => {
        resolveLateFirst(lateFirst);
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });

      expect(container.textContent).toContain('Agent B current');
      expect(container.textContent).not.toContain('Agent A late refresh');
    } finally {
      await unmount(root);
    }
  });

  it('coalesces an Agent-branch session-envelope burst into one trailing refresh', async () => {
    const branch = {
      ...session([event('agent-burst-initial', 1, 'textDelta', 'Agent burst initial')]),
      branchId: 'agent-burst',
    };
    const intermediate = {
      ...session([event('agent-burst-intermediate', 2, 'textDelta', 'Agent burst intermediate')]),
      branchId: 'agent-burst',
    };
    const latest = {
      ...session([event('agent-burst-latest', 3, 'textDelta', 'Agent burst latest')]),
      branchId: 'agent-burst',
    };
    let resolveIntermediate!: (value: AcpSessionVm) => void;
    const pendingIntermediate = new Promise<AcpSessionVm>((resolve) => {
      resolveIntermediate = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(branch)
      .mockReturnValueOnce(pendingIntermediate)
      .mockResolvedValue(latest);

    const { container, root } = await renderDialog(branch, 'agent-burst');
    try {
      await act(async () => {
        const envelope = {
          ...locator,
          branchId: 'agent-burst',
          session: branch,
        };
        runtime.listener?.(envelope);
        runtime.listener?.(envelope);
        runtime.listener?.(envelope);
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);

      await act(async () => {
        resolveIntermediate(intermediate);
        await vi.waitFor(() => expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(3));
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });

      expect(container.textContent).toContain('Agent burst latest');
      expect(container.textContent).not.toContain('Agent burst intermediate');
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
    const pageSize = 30;
    const loadedWindowSize = loadedEventBufferLimit(pageSize);
    const oldestPrompt = event('prompt-live-window', 1, 'userTextDelta', '窗口中最早的消息', {
      raw: { source: 'goldBandPrompt', promptId: 'prompt-live-window' },
    });
    const initial = session([oldestPrompt]);
    vi.mocked(getAcpSession).mockResolvedValue(initial);

    const { container, root } = await renderDialog(
      initial,
      'root',
      undefined,
      undefined,
      locator,
      pageSize,
    );
    try {
      await act(async () => {
        for (let index = 0; index < loadedWindowSize; index += 1) {
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

      await act(async () => {
        scroller!.scrollTop = 1_600;
        scroller!.dispatchEvent(new WheelEvent('wheel', {
          bubbles: true,
          deltaY: -100,
        }));
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 30));
      });

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

  it('atomically rejoins when newer pagination reaches the server head while the viewport stays at bottom', async () => {
    const pageSize = 30;
    const current = session([
      event('auto-rejoin-history', 1, 'textDelta', '自动交接前的旧窗口'),
    ]);
    Object.assign(current.eventPage, {
      coveredRevision: 2,
      newestRevision: 1,
      total: 2,
      hasOlder: true,
      hasNewer: true,
    });
    const serverHead = session([
      event('auto-rejoin-stale-head', 2, 'textDelta', '服务端页中的滞后内容'),
    ]);
    Object.assign(serverHead.eventPage, {
      coveredRevision: 2,
      newestRevision: 2,
      total: 2,
      hasOlder: true,
      hasNewer: false,
    });
    let resolveNewer!: (value: AcpSessionVm) => void;
    const pendingNewer = new Promise<AcpSessionVm>((resolve) => {
      resolveNewer = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(current)
      .mockReturnValueOnce(pendingNewer)
      .mockResolvedValue(serverHead);

    const { container, root } = await renderDialog(
      current,
      'root',
      undefined,
      undefined,
      locator,
      pageSize,
    );
    try {
      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto'));
      expect(scroller).toBeDefined();
      Object.defineProperties(scroller!, {
        clientHeight: { configurable: true, value: 600 },
        scrollHeight: { configurable: true, value: 2_400 },
        scrollTop: { configurable: true, value: 1_800, writable: true },
      });

      await act(async () => {
        scroller!.dispatchEvent(new Event('scroll'));
        await vi.waitFor(() => expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2));
      });
      await act(async () => {
        runtime.listener?.(update(event(
          'auto-rejoin-live-head',
          3,
          'textDelta',
          '分页请求期间到达的最终内容',
        )));
        await new Promise((resolve) => window.setTimeout(resolve, 180));
        Object.defineProperty(scroller!, 'scrollHeight', {
          configurable: true,
          value: 2_600,
        });
        resolveNewer(serverHead);
        await new Promise((resolve) => window.setTimeout(resolve, 300));
      });

      expect(container.textContent).toContain('分页请求期间到达的最终内容');
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
      expect(scroller!.scrollTop).toBe(2_000);
      expect(readConversationBranchReplaySnapshot(locator, 'root').events).toEqual([]);
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(3);
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

  it('settles optimistic prompt admission before a newer-generation event is gated from history', async () => {
    const turnId = 'prompt-accepted-while-reading-history';
    const historical = session([
      event('historical-message', 1, 'textDelta', '正在阅读的历史消息'),
    ]);
    Object.assign(historical.eventPage, {
      coveredRevision: 1,
      newestRevision: 1,
      total: 2,
      hasNewer: true,
    });
    const optimistic = optimisticUserEvent('刚刚发送的新问题', turnId, [], 1);
    const optimisticSessionKey = createAcpSessionCacheKey(
      undefined,
      locator.taskId,
      locator.runId,
      locator.roundId,
      locator.nodeId,
      locator.attemptId,
      locator.projectId,
      undefined,
      undefined,
      'root',
    );
    updateAcpOptimisticEvents(optimisticSessionKey, () => [optimistic]);
    vi.mocked(getAcpSession).mockResolvedValue(historical);

    const { container, root } = await renderDialog(
      historical,
      'root',
      undefined,
      [optimistic],
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
        scrollTop: { configurable: true, value: 500, writable: true },
      });
      await detachConversationViewport(container);
      const visibleItemCount = container.querySelectorAll('[data-acp-item-key]').length;
      expect(container.textContent).toContain('发送中');
      expect(container.textContent).toContain('刚刚发送的新问题');

      await act(async () => {
        runtime.listener?.({
          ...update(event(
            'canonical-current-prompt',
            2,
            'userTextDelta',
            '刚刚发送的新问题',
            { raw: { source: 'goldBandPrompt', promptId: turnId } },
          )),
          timelineGeneration: 2,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });

      expect(readConversationBranchReplaySnapshot(locator, 'root').events)
        .toEqual([expect.objectContaining({ id: 'canonical-current-prompt' })]);
      expect(container.querySelectorAll('[data-acp-item-key]')).toHaveLength(visibleItemCount);
      expect(container.textContent).toContain('刚刚发送的新问题');
      expect(container.textContent).not.toContain('发送中');
      expect(scroller!.scrollTop).toBe(500);
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).not.toBeNull();
    } finally {
      updateAcpOptimisticEvents(optimisticSessionKey, () => []);
      await unmount(root);
    }
  });

  it('shows return-to-latest only after the detached viewport crosses the distance threshold', async () => {
    const canonicalHead = session([
      event('canonical-head-message', 10, 'textDelta', '已经位于 canonical head'),
    ]);
    Object.assign(canonicalHead.eventPage, {
      generation: 1,
      coveredRevision: 10,
      newestRevision: 10,
      oldestSeq: 10,
      newestSeq: 10,
      total: 1,
      hasOlder: true,
      hasNewer: false,
    });
    const metadataRefresh = {
      ...canonicalHead,
      diagnostics: {
        ...canonicalHead.diagnostics,
        rawFrameCount: canonicalHead.diagnostics.rawFrameCount + 1,
      },
    };
    vi.mocked(getAcpSession).mockResolvedValue(canonicalHead);
    const onAtBottomChange = vi.fn();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    await act(async () => {
      root.render(
        <TooltipProvider>
          <ACPChatDialog
            session={canonicalHead}
            {...locator}
            branchId="root"
            onAtBottomChange={onAtBottomChange}
            showSystemPromptAction={false}
            showRawFramesAction={false}
            usageCompact
          />
        </TooltipProvider>,
      );
      await new Promise((resolve) => window.setTimeout(resolve, 0));
    });

    try {
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto'));
      expect(scroller).toBeDefined();
      Object.defineProperties(scroller!, {
        clientHeight: { configurable: true, value: 600 },
        scrollHeight: { configurable: true, value: 2_400 },
        scrollTop: { configurable: true, value: 1_720, writable: true },
      });

      await act(async () => {
        scroller!.dispatchEvent(new WheelEvent('wheel', {
          bubbles: true,
          deltaY: -100,
        }));
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });
      expect(onAtBottomChange).toHaveBeenLastCalledWith(false);
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();

      await act(async () => {
        scroller!.scrollTop = 1_600;
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });
      const returnToLatestBeforeRefresh = container.querySelector<HTMLButtonElement>(
        '[data-acp-return-to-latest="true"]',
      );
      expect(returnToLatestBeforeRefresh).not.toBeNull();
      expect(streamingDiagnostics.records).toContainEqual({
        stage: 'return-to-latest-trace',
        details: expect.objectContaining({
          event: 'visibility-change',
          source: 'viewport-scroll',
          previousVisible: false,
          nextVisible: true,
          distanceFromBottom: 200,
          viewportManualIntent: true,
        }),
      });
      const attachRecord = streamingDiagnostics.records.find(
        (record) => record.details.event === 'dom-attach',
      );
      expect(attachRecord?.details).toEqual(expect.objectContaining({
        visible: true,
        mountSequence: 1,
      }));

      await act(async () => {
        scroller!.scrollTop = 1_684;
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });
      expect(container.querySelector('[data-acp-return-to-latest="true"]'))
        .toBe(returnToLatestBeforeRefresh);
      expect(streamingDiagnostics.records).not.toContainEqual({
        stage: 'return-to-latest-trace',
        details: expect.objectContaining({
          event: 'visibility-change',
          previousVisible: true,
          nextVisible: false,
          distanceFromBottom: 116,
        }),
      });

      await act(async () => {
        scroller!.scrollTop = 1_752;
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });
      expect(container.querySelector('[data-acp-return-to-latest="true"]'))
        .toBe(returnToLatestBeforeRefresh);

      await act(async () => {
        Object.defineProperty(scroller!, 'scrollHeight', {
          configurable: true,
          value: 2_520,
        });
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });
      expect(container.querySelector('[data-acp-return-to-latest="true"]'))
        .toBe(returnToLatestBeforeRefresh);

      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          timelineGeneration: 1,
          timelineRevision: 10,
          session: metadataRefresh,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });

      expect(container.textContent).toContain('已经位于 canonical head');
      const returnToLatest = container.querySelector<HTMLButtonElement>(
        '[data-acp-return-to-latest="true"]',
      );
      expect(returnToLatest).not.toBeNull();
      expect(returnToLatest).toBe(returnToLatestBeforeRefresh);
      expect(streamingDiagnostics.records.filter(
        (record) => record.details.event === 'dom-attach',
      )).toHaveLength(1);
      expect(streamingDiagnostics.records.filter(
        (record) => record.details.event === 'dom-detach',
      )).toHaveLength(0);

      await act(async () => {
        returnToLatest!.click();
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });

      expect(onAtBottomChange).toHaveBeenLastCalledWith(true);
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
      expect(streamingDiagnostics.records).toContainEqual({
        stage: 'return-to-latest-trace',
        details: expect.objectContaining({
          event: 'dom-detach',
          mountSequence: 1,
        }),
      });
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(1);
    } finally {
      await unmount(root);
    }
  });

  it('keeps return-to-latest mounted until a downward user scroll settles at canonical head', async () => {
    const canonicalHead = session([
      event('streaming-canonical-head', 10, 'textDelta', '流式 canonical head'),
    ]);
    vi.mocked(getAcpSession).mockResolvedValue(canonicalHead);

    const { container, root } = await renderDialog(canonicalHead);
    try {
      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto'));
      expect(scroller).toBeDefined();
      Object.defineProperties(scroller!, {
        clientHeight: { configurable: true, value: 600 },
        scrollHeight: { configurable: true, value: 2_400 },
        scrollTop: { configurable: true, value: 1_600, writable: true },
      });

      await act(async () => {
        scroller!.dispatchEvent(new WheelEvent('wheel', {
          bubbles: true,
          deltaY: -100,
        }));
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });
      const returnToLatest = container.querySelector<HTMLButtonElement>(
        '[data-acp-return-to-latest="true"]',
      );
      expect(returnToLatest).not.toBeNull();

      await act(async () => {
        scroller!.dispatchEvent(new WheelEvent('wheel', {
          bubbles: true,
          deltaY: 100,
        }));
        scroller!.scrollTop = 1_800;
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });

      expect(container.querySelector('[data-acp-return-to-latest="true"]'))
        .toBe(returnToLatest);

      await act(async () => {
        scroller!.dispatchEvent(new Event('scrollend'));
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
    } finally {
      await unmount(root);
    }
  });

  it('keeps return-to-latest mounted at a loaded window bottom while canonical newer events remain', async () => {
    const historicalWindow = session([
      event('historical-window-message', 10, 'textDelta', '仍有 newer page 的历史窗口'),
    ]);
    Object.assign(historicalWindow.eventPage, {
      generation: 1,
      coveredRevision: 20,
      newestRevision: 20,
      oldestSeq: 10,
      newestSeq: 10,
      total: 20,
      hasOlder: true,
      hasNewer: true,
    });
    const pendingNewerPage = new Promise<AcpSessionVm>(() => undefined);
    vi.mocked(getAcpSession).mockImplementation(async (...args) => (
      typeof args[6]?.afterSeq === 'number' ? pendingNewerPage : historicalWindow
    ));
    const onAtBottomChange = vi.fn();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    await act(async () => {
      root.render(
        <TooltipProvider>
          <ACPChatDialog
            session={historicalWindow}
            {...locator}
            branchId="root"
            onAtBottomChange={onAtBottomChange}
            showSystemPromptAction={false}
            showRawFramesAction={false}
            usageCompact
          />
        </TooltipProvider>,
      );
      await new Promise((resolve) => window.setTimeout(resolve, 0));
    });

    try {
      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto'));
      expect(scroller).toBeDefined();
      Object.defineProperties(scroller!, {
        clientHeight: { configurable: true, value: 600 },
        scrollHeight: { configurable: true, value: 2_400 },
        scrollTop: { configurable: true, value: 1_600, writable: true },
      });

      await act(async () => {
        scroller!.dispatchEvent(new WheelEvent('wheel', {
          bubbles: true,
          deltaY: -100,
        }));
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });
      const returnToLatest = container.querySelector<HTMLButtonElement>(
        '[data-acp-return-to-latest="true"]',
      );
      expect(returnToLatest).not.toBeNull();

      await act(async () => {
        scroller!.dispatchEvent(new WheelEvent('wheel', {
          bubbles: true,
          deltaY: 100,
        }));
        scroller!.scrollTop = 1_800;
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });

      expect(onAtBottomChange).toHaveBeenLastCalledWith(false);
      expect(container.querySelector('[data-acp-return-to-latest="true"]'))
        .toBe(returnToLatest);
      expect(streamingDiagnostics.records).not.toContainEqual({
        stage: 'return-to-latest-trace',
        details: expect.objectContaining({
          event: 'visibility-change',
          source: 'at-bottom-change',
          previousVisible: true,
          nextVisible: false,
          viewportAtBottom: true,
          hasNewerEvents: true,
        }),
      });
    } finally {
      await unmount(root);
    }
  });

  it('keeps return-to-latest enabled while automatic newer-edge catch-up is pending', async () => {
    const historicalWindow = session([
      event('automatic-catch-up-history', 1, 'textDelta', '自动追头前的历史窗口'),
    ]);
    Object.assign(historicalWindow.eventPage, {
      coveredRevision: 2,
      newestRevision: 2,
      total: 2,
      hasNewer: true,
    });
    const newerEdge = session([
      event('automatic-catch-up-edge', 2, 'textDelta', '自动分页抵达的新边界'),
    ]);
    Object.assign(newerEdge.eventPage, {
      coveredRevision: 2,
      newestRevision: 2,
      total: 2,
      hasOlder: true,
      hasNewer: false,
    });
    let resolveCanonicalHead!: (value: AcpSessionVm) => void;
    const pendingCanonicalHead = new Promise<AcpSessionVm>((resolve) => {
      resolveCanonicalHead = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(historicalWindow)
      .mockResolvedValueOnce(newerEdge)
      .mockReturnValueOnce(pendingCanonicalHead);

    const { container, root } = await renderDialog(historicalWindow);
    try {
      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto'));
      expect(scroller).toBeDefined();
      Object.defineProperties(scroller!, {
        clientHeight: { configurable: true, value: 600 },
        scrollHeight: { configurable: true, value: 2_400 },
        scrollTop: { configurable: true, value: 1_650, writable: true },
      });

      await act(async () => {
        scroller!.dispatchEvent(new WheelEvent('wheel', {
          bubbles: true,
          deltaY: -1,
        }));
        scroller!.dispatchEvent(new Event('scroll'));
        await vi.waitFor(() => {
          expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(3);
        });
      });

      const returnToLatest = container.querySelector<HTMLButtonElement>(
        '[data-acp-return-to-latest="true"]',
      );
      expect(returnToLatest).not.toBeNull();
      expect(returnToLatest?.disabled).toBe(false);
    } finally {
      await act(async () => {
        resolveCanonicalHead(newerEdge);
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
      await unmount(root);
    }
  });

  it('disables return-to-latest while an explicit user catch-up is pending', async () => {
    const historicalWindow = session([
      event('manual-catch-up-history', 1, 'textDelta', '手动追头前的历史窗口'),
    ]);
    Object.assign(historicalWindow.eventPage, {
      coveredRevision: 2,
      newestRevision: 2,
      total: 2,
      hasNewer: true,
    });
    const canonicalHead = session([
      event('manual-catch-up-head', 2, 'textDelta', '手动抵达的最新内容'),
    ]);
    Object.assign(canonicalHead.eventPage, {
      coveredRevision: 2,
      newestRevision: 2,
      total: 2,
      hasOlder: true,
      hasNewer: false,
    });
    let resolveCanonicalHead!: (value: AcpSessionVm) => void;
    const pendingCanonicalHead = new Promise<AcpSessionVm>((resolve) => {
      resolveCanonicalHead = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(historicalWindow)
      .mockReturnValueOnce(pendingCanonicalHead);

    const { container, root } = await renderDialog(historicalWindow);
    try {
      await detachConversationViewport(container);
      const returnToLatest = container.querySelector<HTMLButtonElement>(
        '[data-acp-return-to-latest="true"]',
      );
      expect(returnToLatest).not.toBeNull();

      await act(async () => {
        returnToLatest!.click();
        await vi.waitFor(() => {
          expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
        });
      });

      expect(returnToLatest?.disabled).toBe(true);

      await act(async () => {
        resolveCanonicalHead(canonicalHead);
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
    } finally {
      await unmount(root);
    }
  });

  it('does not rewrite follow intent for every scroll while a newer window remains', async () => {
    const historicalWindow = session([
      event('historical-scroll-message', 10, 'textDelta', '仍有 newer page 的历史窗口'),
    ]);
    Object.assign(historicalWindow.eventPage, {
      generation: 1,
      coveredRevision: 20,
      newestRevision: 20,
      oldestSeq: 10,
      newestSeq: 10,
      total: 20,
      hasOlder: true,
      hasNewer: true,
    });
    vi.mocked(getAcpSession).mockResolvedValue(historicalWindow);

    const { container, root } = await renderDialog(historicalWindow);
    try {
      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto'));
      expect(scroller).toBeDefined();
      Object.defineProperties(scroller!, {
        clientHeight: { configurable: true, value: 600 },
        scrollHeight: { configurable: true, value: 2_400 },
        scrollTop: { configurable: true, value: 1_000, writable: true },
      });
      await act(async () => {
        scroller!.dispatchEvent(new WheelEvent('wheel', {
          bubbles: true,
          deltaY: -1,
        }));
        scroller!.scrollTop = 1_000;
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });
      streamingDiagnostics.enabled = true;
      streamingDiagnostics.records = [];

      await act(async () => {
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 30));
        scroller!.scrollTop = 1_010;
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 30));
        scroller!.scrollTop = 1_020;
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 30));
      });

      const externalStops = streamingDiagnostics.records.filter((record) => (
        record.stage === 'chat-scroll-trace'
        && record.details.event === 'follow-write'
        && record.details.cause === 'external-stop-scroll'
      ));
      expect(externalStops.map((record) => record.details)).toEqual([]);
    } finally {
      await unmount(root);
    }
  });

  it('keeps a stored accepted prompt static through terminal lifecycle until the canonical head replaces it', async () => {
    const turnId = 'controlled-prompt-accepted-in-history';
    const historical = session([
      event('controlled-history', 1, 'textDelta', '受控状态正在阅读的历史消息'),
    ]);
    Object.assign(historical.eventPage, {
      coveredRevision: 1,
      newestRevision: 1,
      total: 2,
      hasNewer: true,
    });
    const optimistic = optimisticUserEvent('受控状态的新问题', turnId, [], 1);
    const canonicalPrompt = event(
      'controlled-canonical-prompt',
      2,
      'userTextDelta',
      '受控状态的新问题',
      { raw: { source: 'goldBandPrompt', promptId: turnId } },
    );
    const latest = session([canonicalPrompt]);
    Object.assign(latest.eventPage, {
      coveredRevision: 2,
      newestRevision: 2,
      total: 2,
      hasOlder: true,
      hasNewer: false,
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(historical)
      .mockResolvedValueOnce(latest);

    const { container, root } = await renderStoredOptimisticDialog(
      historical,
      [optimistic],
      30,
    );
    try {
      await detachConversationViewport(container);
      const optimisticItem = [...container.querySelectorAll<HTMLElement>('[data-acp-item-key]')]
        .find((item) => item.textContent?.includes('受控状态的新问题'));
      expect(optimisticItem).toBeDefined();
      const optimisticKey = optimisticItem?.dataset.acpItemKey;
      expect(optimisticKey).toBeTruthy();
      expect(container.textContent).toContain('发送中');

      await act(async () => {
        runtime.listener?.(update(canonicalPrompt));
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });

      expect(container.querySelector(`[data-acp-item-key="${optimisticKey}"]`))
        .toBe(optimisticItem);
      expect(container.textContent).toContain('受控状态的新问题');
      expect(container.textContent).not.toContain('发送中');
      expect(container.textContent).not.toContain('controlled-canonical-prompt');

      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          timelineGeneration: 1,
          timelineRevision: 2,
          lifecycle: terminalLifecycle(turnId),
        });
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      expect(container.querySelector(`[data-acp-item-key="${optimisticKey}"]`))
        .toBe(optimisticItem);
      expect(container.textContent).toContain('受控状态的新问题');

      await act(async () => {
        container.querySelector<HTMLButtonElement>(
          '[data-acp-return-to-latest="true"]',
        )?.click();
        await new Promise((resolve) => window.setTimeout(resolve, 300));
      });

      const promptItems = [...container.querySelectorAll<HTMLElement>('[data-acp-item-key]')]
        .filter((item) => item.textContent?.includes('受控状态的新问题'));
      expect(promptItems).toHaveLength(1);
      expect(promptItems[0]?.dataset.acpItemKey).toBe(
        'userTextDelta-controlled-canonical-prompt',
      );
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
    } finally {
      await unmount(root);
    }
  });

  it('keeps a submitted prompt static when its canonical submit response arrives over a historical window', async () => {
    const historical = session([
      event('submit-history', 1, 'textDelta', '提交响应到达前的历史窗口'),
    ], 'completed');
    Object.assign(historical.eventPage, {
      coveredRevision: 1,
      newestRevision: 1,
      total: 2,
      hasNewer: true,
    });
    vi.mocked(getAcpSession).mockResolvedValue(historical);
    vi.mocked(submitConversationPrompt).mockImplementation(async (...args) => {
      const promptId = String(args[7]);
      const canonical = event('submit-canonical-prompt', 2, 'userTextDelta', '历史窗口内发送的问题', {
        raw: { source: 'goldBandPrompt', promptId },
      });
      const updated = session([canonical]);
      Object.assign(updated.eventPage, {
        coveredRevision: 2,
        newestRevision: 2,
        total: 2,
        hasOlder: true,
        hasNewer: false,
      });
      return {
        kind: 'acp-session',
        session: updated,
        run: null,
        lifecycle: null,
      };
    });

    const { container, root } = await renderDialog(
      historical,
      'root',
      undefined,
      undefined,
      locator,
      30,
      terminalLifecycle('previous-submit-turn'),
    );
    try {
      const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
      expect(textarea).not.toBeNull();
      await setTextareaValue(textarea!, '历史窗口内发送的问题');
      await act(async () => {
        container.querySelector<HTMLButtonElement>('[data-acp-send="true"]')?.click();
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });

      const promptItems = [...container.querySelectorAll<HTMLElement>('[data-acp-item-key]')]
        .filter((item) => item.textContent?.includes('历史窗口内发送的问题'));
      expect(promptItems).toHaveLength(1);
      expect(promptItems[0]?.dataset.acpItemKey).not.toBe(
        'userTextDelta-submit-canonical-prompt',
      );
      expect(container.textContent).not.toContain('发送中');
      expect(container.textContent).toContain('提交响应到达前的历史窗口');
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
    } finally {
      await unmount(root);
    }
  });

  it('does not regress an admitted prompt to failed when its submit transport rejects late', async () => {
    const historical = session([
      event('late-reject-history', 1, 'textDelta', '迟到失败前的历史窗口'),
    ], 'completed');
    Object.assign(historical.eventPage, {
      coveredRevision: 1,
      newestRevision: 1,
      total: 2,
      hasNewer: true,
    });
    let rejectSubmit!: (reason: Error) => void;
    vi.mocked(getAcpSession).mockResolvedValue(historical);
    vi.mocked(submitConversationPrompt).mockReturnValue(new Promise((_, reject) => {
      rejectSubmit = reject;
    }));

    const { container, root } = await renderDialog(
      historical,
      'root',
      undefined,
      undefined,
      locator,
      30,
      terminalLifecycle('previous-late-reject-turn'),
    );
    try {
      const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
      await setTextareaValue(textarea!, '已经被 canonical 接收的问题');
      await act(async () => {
        container.querySelector<HTMLButtonElement>('[data-acp-send="true"]')?.click();
        await vi.waitFor(() => expect(vi.mocked(submitConversationPrompt)).toHaveBeenCalledTimes(1));
      });
      const promptId = String(vi.mocked(submitConversationPrompt).mock.calls[0]?.[7]);

      await act(async () => {
        runtime.listener?.(update(event(
          'late-reject-canonical-prompt',
          2,
          'userTextDelta',
          '已经被 canonical 接收的问题',
          { raw: { source: 'goldBandPrompt', promptId } },
        )));
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });
      expect(container.textContent).not.toContain('发送中');

      await act(async () => {
        rejectSubmit(new Error('late transport rejection'));
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });

      expect(container.textContent).toContain('已经被 canonical 接收的问题');
      expect(container.textContent).not.toContain('late transport rejection');
      expect(textarea?.value).toBe('');
    } finally {
      await unmount(root);
    }
  });

  it('keeps the visible historical window when the same-session prop refresh carries the live head', async () => {
    const historical = session([
      event('prop-history', 1, 'textDelta', 'prop 刷新前的历史锚点'),
    ]);
    Object.assign(historical.eventPage, {
      coveredRevision: 1,
      newestRevision: 1,
      total: 3,
      hasOlder: true,
      hasNewer: true,
    });
    const head = session([
      event('prop-head-prompt', 2, 'userTextDelta', '不应注入的当前问题', {
        raw: { source: 'goldBandPrompt', promptId: 'prop-head-prompt' },
      }),
      event('prop-head-answer', 3, 'textDelta', '不应注入的当前回复'),
    ]);
    Object.assign(head.eventPage, {
      generation: 2,
      coveredRevision: 3,
      newestRevision: 3,
      total: 3,
      hasOlder: true,
      hasNewer: false,
    });
    vi.mocked(getAcpSession).mockResolvedValue(historical);

    const { container, root } = await renderDialog(historical, 'root', undefined, undefined, locator, 30);
    try {
      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto'));
      expect(scroller).toBeDefined();
      Object.defineProperties(scroller!, {
        clientHeight: { configurable: true, value: 600 },
        scrollHeight: { configurable: true, value: 2_400 },
        scrollTop: { configurable: true, value: 500, writable: true },
      });
      await detachConversationViewport(container);
      const historicalItem = [...container.querySelectorAll<HTMLElement>('[data-acp-item-key]')]
        .find((item) => item.textContent?.includes('prop 刷新前的历史锚点'));
      expect(historicalItem).toBeDefined();

      await act(async () => {
        root.render(
          <TooltipProvider>
            <ACPChatDialog
              session={head}
              {...locator}
              branchId="root"
              eventPageSize={30}
              showSystemPromptAction={false}
              showRawFramesAction={false}
              usageCompact
            />
          </TooltipProvider>,
        );
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      expect(container.textContent).toContain('prop 刷新前的历史锚点');
      expect(container.textContent).not.toContain('不应注入的当前问题');
      expect(container.textContent).not.toContain('不应注入的当前回复');
      expect([...container.querySelectorAll<HTMLElement>('[data-acp-item-key]')]
        .find((item) => item.textContent?.includes('prop 刷新前的历史锚点')))
        .toBe(historicalItem);
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).not.toBeNull();
      expect(scroller!.scrollTop).toBe(500);
    } finally {
      await unmount(root);
    }
  });

  it('replaces the live-head DOM when a same-session prop advances generation', async () => {
    const previous = session([
      event('prop-generation-old', 1, 'textDelta', 'prop 一代旧内容'),
    ]);
    const current = session([
      event('prop-generation-current', 1, 'textDelta', 'prop 二代当前内容'),
    ]);
    current.eventPage.generation = 2;
    current.eventPage.coveredRevision = 1;
    vi.mocked(getAcpSession).mockResolvedValue(previous);

    const { container, root } = await renderDialog(previous);
    try {
      expect(container.textContent).toContain('prop 一代旧内容');
      await act(async () => {
        root.render(
          <TooltipProvider>
            <ACPChatDialog
              session={current}
              {...locator}
              branchId="root"
              showSystemPromptAction={false}
              showRawFramesAction={false}
              usageCompact
            />
          </TooltipProvider>,
        );
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      expect(container.textContent).toContain('prop 二代当前内容');
      expect(container.textContent).not.toContain('prop 一代旧内容');
    } finally {
      await unmount(root);
    }
  });

  it('preserves event-only usage metadata across the latest-head replay handoff', async () => {
    const historical = session([
      event('usage-history', 1, 'textDelta', 'usage 交接前的历史窗口'),
    ]);
    Object.assign(historical.eventPage, {
      coveredRevision: 1,
      newestRevision: 1,
      total: 2,
      hasNewer: true,
    });
    const staleLatest = session([
      event('usage-head', 2, 'textDelta', 'usage 交接后的最新正文'),
    ]);
    Object.assign(staleLatest.eventPage, {
      coveredRevision: 2,
      newestRevision: 2,
      total: 2,
      hasOlder: true,
      hasNewer: false,
    });
    let resolveLatest!: (value: AcpSessionVm) => void;
    const pendingLatest = new Promise<AcpSessionVm>((resolve) => {
      resolveLatest = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(historical)
      .mockReturnValueOnce(pendingLatest);

    const { container, root } = await renderDialog(historical, 'root', undefined, undefined, locator, 30);
    try {
      await detachConversationViewport(container);
      await act(async () => {
        container.querySelector<HTMLButtonElement>(
          '[data-acp-return-to-latest="true"]',
        )?.click();
        await vi.waitFor(() => expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2));
      });

      await act(async () => {
        runtime.listener?.(update(event('usage-live', 3, 'usageUpdate', null, {
          raw: { sessionUpdate: 'usage_update', used: 7_920, size: 258_400 },
        })));
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });
      const usageLabel = container.querySelector<HTMLElement>(
        '[data-context-usage-gauge="true"]',
      )?.getAttribute('aria-label');
      expect(usageLabel).toContain('7.9K');

      await act(async () => {
        resolveLatest(staleLatest);
        await new Promise((resolve) => window.setTimeout(resolve, 300));
      });

      expect(container.textContent).toContain('usage 交接后的最新正文');
      expect(container.querySelector<HTMLElement>(
        '[data-context-usage-gauge="true"]',
      )?.getAttribute('aria-label')).toBe(usageLabel);
      expect(readConversationBranchReplaySnapshot(locator, 'root').events).toEqual([]);
    } finally {
      await unmount(root);
    }
  });

  it('rejects a late latest-head request after the selected session identity changes', async () => {
    const historical = session([
      event('identity-old-history', 1, 'textDelta', '旧会话历史窗口'),
    ]);
    Object.assign(historical.eventPage, {
      coveredRevision: 1,
      newestRevision: 1,
      total: 2,
      hasNewer: true,
    });
    const staleOldHead = session([
      event('identity-old-head', 2, 'textDelta', '迟到的旧会话最新内容'),
    ]);
    Object.assign(staleOldHead.eventPage, {
      coveredRevision: 2,
      newestRevision: 2,
      total: 2,
      hasNewer: false,
    });
    const nextLocator = { ...locator, nodeId: 'identity-next-node', attemptId: 'identity-next-attempt' };
    const nextSession = {
      ...session([event('identity-next-content', 1, 'textDelta', '新会话保持可见')]),
      nodeId: nextLocator.nodeId,
      attemptId: nextLocator.attemptId,
    };
    let resolveOldLatest!: (value: AcpSessionVm) => void;
    let oldRequestCount = 0;
    vi.mocked(getAcpSession).mockImplementation(async (...args) => {
      if (args[4] === nextLocator.nodeId) return nextSession;
      oldRequestCount += 1;
      if (oldRequestCount === 1) return historical;
      return new Promise<AcpSessionVm>((resolve) => {
        resolveOldLatest = resolve;
      });
    });

    const { container, root } = await renderDialog(historical, 'root', undefined, undefined, locator, 30);
    try {
      await detachConversationViewport(container);
      await act(async () => {
        container.querySelector<HTMLButtonElement>(
          '[data-acp-return-to-latest="true"]',
        )?.click();
        await vi.waitFor(() => expect(oldRequestCount).toBe(2));
      });

      await act(async () => {
        root.render(
          <TooltipProvider>
            <ACPChatDialog
              session={nextSession}
              {...nextLocator}
              branchId="root"
              eventPageSize={30}
              showSystemPromptAction={false}
              showRawFramesAction={false}
              usageCompact
            />
          </TooltipProvider>,
        );
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
      expect(container.textContent).toContain('新会话保持可见');

      await act(async () => {
        resolveOldLatest(staleOldHead);
        await new Promise((resolve) => window.setTimeout(resolve, 300));
      });

      expect(container.textContent).toContain('新会话保持可见');
      expect(container.textContent).not.toContain('迟到的旧会话最新内容');
    } finally {
      await unmount(root);
    }
  });

  it('atomically rejoins the live head with replay received during the latest-page request', async () => {
    const historical = session([
      event('historical-answer', 1, 'textDelta', '当前阅读的旧回复'),
    ]);
    Object.assign(historical.eventPage, {
      coveredRevision: 1,
      newestRevision: 1,
      total: 3,
      hasOlder: true,
      hasNewer: true,
    });
    const prompt = event('current-prompt', 2, 'userTextDelta', '当前问题', {
      raw: { source: 'goldBandPrompt', promptId: 'current-prompt' },
    });
    const staleLatest = session([
      prompt,
      event('current-answer', 3, 'textDelta', '后端页中的旧累计回复', {
        startedSeq: 3,
        endedSeq: 3,
      }),
      event('current-tool', 3, 'toolCall', null, {
        title: 'Reading files',
        toolCallId: 'current-tool',
        status: 'running',
      }),
    ]);
    Object.assign(staleLatest.eventPage, {
      coveredRevision: 3,
      newestRevision: 3,
      total: 3,
      hasOlder: true,
      hasNewer: false,
    });
    let resolveLatest!: (value: AcpSessionVm) => void;
    const pendingLatest = new Promise<AcpSessionVm>((resolve) => {
      resolveLatest = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(historical)
      .mockReturnValueOnce(pendingLatest);

    const { container, root } = await renderDialog(historical);
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
        scrollTop: { configurable: true, value: 500, writable: true },
      });
      await detachConversationViewport(container);
      vi.stubGlobal('requestAnimationFrame', vi.fn(() => 1));
      vi.stubGlobal('cancelAnimationFrame', vi.fn());
      const returnButton = container.querySelector<HTMLButtonElement>(
        '[data-acp-return-to-latest="true"]',
      );
      expect(returnButton).not.toBeNull();

      await act(async () => {
        returnButton!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        await vi.waitFor(() => {
          expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
        });
      });

      await act(async () => {
        runtime.listener?.(update(event(
          'current-answer',
          5,
          'textDelta',
          'replay 中的最终累计回复',
          { startedSeq: 3, endedSeq: 5 },
        )));
        runtime.listener?.(update(event(
          'current-tool',
          5,
          'toolCall',
          null,
          {
            title: 'Editing files',
            toolCallId: 'current-tool',
            status: 'completed',
            startedSeq: 3,
            endedSeq: 5,
          },
        )));
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });
      expect(readConversationBranchReplaySnapshot(locator, 'root').headRevision).toBe(5);

      await act(async () => {
        resolveLatest(staleLatest);
        await new Promise((resolve) => window.setTimeout(resolve, 300));
      });

      expect(container.textContent).toContain('replay 中的最终累计回复');
      expect(container.textContent).not.toContain('后端页中的旧累计回复');
      expect(container.textContent).toContain('Editing · files');
      expect(container.textContent).not.toContain('Reading · files');
      expect(container.querySelectorAll('[data-acp-item-key="textDelta-current-answer"]'))
        .toHaveLength(1);
      const activityTrigger = [...container.querySelectorAll<HTMLButtonElement>('button')]
        .find((button) => button.textContent?.includes('Editing · files'));
      expect(activityTrigger?.getAttribute('aria-expanded')).toBe('false');
      expect(getAcpActivityDetail).not.toHaveBeenCalled();
      expect([...container.querySelectorAll('[data-testid="markdown"]')]
        .every((node) => node.getAttribute('data-streaming') === 'false')).toBe(true);
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
      expect(scroller!.scrollTop).toBe(1_800);
      expect(readConversationBranchReplaySnapshot(locator, 'root')).toMatchObject({
        events: [],
        requiresCatchUp: false,
      });
    } finally {
      await unmount(root);
    }
  });

  it('prefers a newer canonical session snapshot received during the latest-page request', async () => {
    const historical = session([
      event('snapshot-history', 1, 'textDelta', '快照竞态中的旧窗口'),
    ]);
    Object.assign(historical.eventPage, {
      coveredRevision: 1,
      newestRevision: 1,
      total: 2,
      hasNewer: true,
    });
    const prompt = event('snapshot-prompt', 2, 'userTextDelta', '检查 session 快照', {
      raw: { source: 'goldBandPrompt', promptId: 'snapshot-prompt' },
    });
    const staleLatest = session([
      prompt,
      event('snapshot-answer', 3, 'textDelta', '查询返回的滞后 session 内容', {
        startedSeq: 3,
        endedSeq: 3,
      }),
    ]);
    Object.assign(staleLatest.eventPage, {
      coveredRevision: 3,
      newestRevision: 3,
      total: 2,
      hasOlder: true,
      hasNewer: false,
    });
    const liveSnapshot = session([
      prompt,
      event('snapshot-answer', 5, 'textDelta', '订阅收到的较新 canonical session', {
        startedSeq: 3,
        endedSeq: 5,
      }),
    ]);
    Object.assign(liveSnapshot.eventPage, {
      coveredRevision: 5,
      newestRevision: 5,
      total: 2,
      hasOlder: true,
      hasNewer: false,
    });
    let resolveLatest!: (value: AcpSessionVm) => void;
    const pendingLatest = new Promise<AcpSessionVm>((resolve) => {
      resolveLatest = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(historical)
      .mockReturnValueOnce(pendingLatest);

    const { container, root } = await renderDialog(historical);
    try {
      await detachConversationViewport(container);
      const returnButton = container.querySelector<HTMLButtonElement>(
        '[data-acp-return-to-latest="true"]',
      );
      expect(returnButton).not.toBeNull();
      await act(async () => {
        returnButton!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        await vi.waitFor(() => {
          expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
        });
      });

      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          timelineGeneration: 1,
          timelineRevision: 5,
          session: liveSnapshot,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
      expect(container.textContent).not.toContain('订阅收到的较新 canonical session');

      await act(async () => {
        resolveLatest(staleLatest);
        await new Promise((resolve) => window.setTimeout(resolve, 300));
      });

      expect(container.textContent).toContain('订阅收到的较新 canonical session');
      expect(container.textContent).not.toContain('查询返回的滞后 session 内容');
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
    } finally {
      await unmount(root);
    }
  });

  it('preserves newer same-revision session metadata received during the latest-page request', async () => {
    const historical = session([
      event('metadata-race-history', 1, 'textDelta', 'metadata 竞态中的旧窗口'),
    ]);
    Object.assign(historical, {
      sessionUpdatedAt: '2026-08-31T00:00:00Z',
      usage: { used: 1, size: 100 },
    });
    Object.assign(historical.eventPage, {
      coveredRevision: 1,
      newestRevision: 1,
      total: 2,
      hasNewer: true,
    });
    const staleLatest = session([
      event('metadata-race-answer', 3, 'textDelta', '同 revision 的查询内容'),
    ]);
    Object.assign(staleLatest, {
      sessionUpdatedAt: '2026-08-31T00:00:01Z',
      usage: { used: 3, size: 100 },
    });
    Object.assign(staleLatest.eventPage, {
      coveredRevision: 3,
      newestRevision: 3,
      hasOlder: true,
      hasNewer: false,
    });
    const newerMetadata = session([
      event('metadata-race-answer', 3, 'textDelta', '同 revision 的查询内容'),
    ], 'completed');
    Object.assign(newerMetadata, {
      sessionUpdatedAt: '2026-08-31T00:00:02Z',
      usage: { used: 9, size: 100 },
    });
    Object.assign(newerMetadata.eventPage, {
      coveredRevision: 3,
      newestRevision: 3,
      hasOlder: true,
      hasNewer: false,
    });
    let resolveLatest!: (value: AcpSessionVm) => void;
    const pendingLatest = new Promise<AcpSessionVm>((resolve) => {
      resolveLatest = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(historical)
      .mockReturnValueOnce(pendingLatest);
    const cacheKey = createAcpEventWindowCacheKey({
      ...locator,
      branchId: 'root',
    });

    const { container, root } = await renderDialog(historical);
    try {
      await detachConversationViewport(container);
      const returnButton = container.querySelector<HTMLButtonElement>(
        '[data-acp-return-to-latest="true"]',
      );
      expect(returnButton).not.toBeNull();
      await act(async () => {
        returnButton!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        await vi.waitFor(() => {
          expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
        });
      });

      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          timelineGeneration: 1,
          timelineRevision: 3,
          session: newerMetadata,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
      expect(restoreAcpSession(cacheKey)).toMatchObject({
        status: 'completed',
        sessionUpdatedAt: '2026-08-31T00:00:02Z',
        usage: { used: 9, size: 100 },
      });

      await act(async () => {
        resolveLatest(staleLatest);
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      expect(restoreAcpSession(cacheKey)).toMatchObject({
        status: 'completed',
        sessionUpdatedAt: '2026-08-31T00:00:02Z',
        usage: { used: 9, size: 100 },
      });
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
    } finally {
      await unmount(root);
    }
  });

  it('queues one fresh canonical handoff when a newer subscription generation overtakes the latest-page request', async () => {
    const historical = session([
      event('generation-race-history', 1, 'textDelta', 'generation 竞态中的旧窗口'),
    ]);
    Object.assign(historical.eventPage, {
      generation: 1,
      coveredRevision: 1,
      newestRevision: 1,
      total: 2,
      hasNewer: true,
    });
    const staleGenerationOne = session([
      event('generation-race-stale', 3, 'textDelta', '迟到的 generation 1 head'),
    ]);
    Object.assign(staleGenerationOne.eventPage, {
      generation: 1,
      coveredRevision: 3,
      newestRevision: 3,
      hasOlder: true,
      hasNewer: false,
    });
    const subscriptionGenerationTwo = session([
      event('generation-race-subscription', 1, 'textDelta', 'subscription 已进入 generation 2'),
    ]);
    Object.assign(subscriptionGenerationTwo.eventPage, {
      generation: 2,
      coveredRevision: 1,
      newestRevision: 1,
      hasOlder: true,
      hasNewer: false,
    });
    const canonicalGenerationTwo = session([
      event('generation-race-canonical', 2, 'textDelta', 'fresh-read 的 generation 2 canonical head'),
    ]);
    Object.assign(canonicalGenerationTwo.eventPage, {
      generation: 2,
      coveredRevision: 2,
      newestRevision: 2,
      hasOlder: true,
      hasNewer: false,
    });
    let resolveStaleGenerationOne!: (value: AcpSessionVm) => void;
    const pendingStaleGenerationOne = new Promise<AcpSessionVm>((resolve) => {
      resolveStaleGenerationOne = resolve;
    });
    let resolveCanonicalGenerationTwo!: (value: AcpSessionVm) => void;
    const pendingCanonicalGenerationTwo = new Promise<AcpSessionVm>((resolve) => {
      resolveCanonicalGenerationTwo = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(historical)
      .mockReturnValueOnce(pendingStaleGenerationOne)
      .mockReturnValueOnce(pendingCanonicalGenerationTwo);

    const { container, root } = await renderDialog(historical);
    try {
      await detachConversationViewport(container);
      const returnButton = container.querySelector<HTMLButtonElement>(
        '[data-acp-return-to-latest="true"]',
      );
      expect(returnButton).not.toBeNull();
      await act(async () => {
        returnButton!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        await vi.waitFor(() => {
          expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
        });
      });

      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          timelineGeneration: 2,
          timelineRevision: 1,
          session: subscriptionGenerationTwo,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
      expect(container.textContent).toContain('generation 竞态中的旧窗口');
      expect(container.textContent).not.toContain('subscription 已进入 generation 2');

      await act(async () => {
        resolveStaleGenerationOne(staleGenerationOne);
        await vi.waitFor(() => {
          expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(3);
        });
      });
      expect(container.textContent).toContain('generation 竞态中的旧窗口');
      expect(container.textContent).not.toContain('迟到的 generation 1 head');

      await act(async () => {
        resolveCanonicalGenerationTwo(canonicalGenerationTwo);
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      expect(container.textContent).toContain('fresh-read 的 generation 2 canonical head');
      expect(container.textContent).not.toContain('迟到的 generation 1 head');
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
      expect(vi.mocked(getAcpSession).mock.calls.map((call) => call[6]?.afterRevision))
        .toEqual([undefined, undefined, undefined]);
    } finally {
      await unmount(root);
    }
  });

  it('catches up a replay loss before committing the latest head', async () => {
    const historical = session([
      event('loss-history', 1, 'textDelta', '仍在阅读的旧窗口'),
    ]);
    Object.assign(historical.eventPage, {
      coveredRevision: 1,
      newestRevision: 1,
      total: 2,
      hasNewer: true,
    });
    const prompt = event('loss-prompt', 2, 'userTextDelta', '追平丢失水位', {
      raw: { source: 'goldBandPrompt', promptId: 'loss-prompt' },
    });
    const staleLatest = session([
      prompt,
      event('loss-answer', 3, 'textDelta', '追平前的旧内容', {
        startedSeq: 3,
        endedSeq: 3,
      }),
    ]);
    Object.assign(staleLatest.eventPage, {
      coveredRevision: 3,
      newestRevision: 3,
      total: 2,
      hasOlder: true,
      hasNewer: false,
    });
    const caughtUp = session([
      event('loss-answer', 9, 'textDelta', '按 revision 追平后的完整内容', {
        startedSeq: 3,
        endedSeq: 9,
      }),
    ]);
    Object.assign(caughtUp.eventPage, {
      coveredRevision: 100,
      newestRevision: 9,
      total: 100,
      hasOlder: true,
      hasNewer: false,
    });
    let resolveLatest!: (value: AcpSessionVm) => void;
    const pendingLatest = new Promise<AcpSessionVm>((resolve) => {
      resolveLatest = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(historical)
      .mockReturnValueOnce(pendingLatest)
      .mockResolvedValueOnce(caughtUp);

    const { container, root } = await renderDialog(historical);
    try {
      await detachConversationViewport(container);
      const returnButton = container.querySelector<HTMLButtonElement>(
        '[data-acp-return-to-latest="true"]',
      );
      expect(returnButton).not.toBeNull();
      await act(async () => {
        returnButton!.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        await vi.waitFor(() => {
          expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
        });
      });

      await act(async () => {
        runtime.listener?.(update(event(
          'loss-answer',
          9,
          'textDelta',
          '按 revision 追平后的完整内容',
          {
            startedSeq: 3,
            endedSeq: 9,
            raw: { oversized: 'x'.repeat(CONVERSATION_EVENT_REPLAY_LIMITS.eventBytes) },
          },
        )));
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });
      expect(readConversationBranchReplaySnapshot(locator, 'root')).toMatchObject({
        events: [],
        requiresCatchUp: true,
        lossWatermarkRevision: 9,
      });

      await act(async () => {
        resolveLatest(staleLatest);
        await new Promise((resolve) => window.setTimeout(resolve, 300));
      });

      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(3);
      expect(vi.mocked(getAcpSession).mock.calls[2]?.[6]).toMatchObject({
        branchId: 'root',
        afterRevision: 3,
      });
      expect(container.textContent).toContain('按 revision 追平后的完整内容');
      expect(container.textContent).not.toContain('追平前的旧内容');
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
      expect(readConversationBranchReplaySnapshot(locator, 'root')).toMatchObject({
        events: [],
        requiresCatchUp: false,
      });
      const cacheKey = createAcpEventWindowCacheKey({
        ...locator,
        branchId: 'root',
      });
      expect(restoreAcpSession(cacheKey)?.eventPage).toMatchObject({
        coveredRevision: 100,
        newestRevision: 9,
        total: 100,
      });
    } finally {
      await unmount(root);
    }
  });

  it('does not restore a cached event window from an older timeline generation', async () => {
    const previous = session([
      event('cached-generation-old', 1, 'textDelta', '缓存中的一代旧内容'),
    ]);
    const current = session([
      event('cached-generation-current', 1, 'textDelta', '重挂载后的二代当前内容'),
    ]);
    current.eventPage.generation = 2;
    current.eventPage.coveredRevision = 1;
    current.eventPage.newestRevision = 1;
    vi.mocked(getAcpSession).mockResolvedValue(previous);

    const first = await renderDialog(previous);
    await unmount(first.root);

    vi.mocked(getAcpSession).mockResolvedValue(current);
    const second = await renderDialog(current);
    try {
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 30));
      });
      expect(second.container.textContent).toContain('重挂载后的二代当前内容');
      expect(second.container.textContent).not.toContain('缓存中的一代旧内容');
    } finally {
      await unmount(second.root);
    }
  });

  it('keeps the historical window isolated when older pagination crosses compaction', async () => {
    const previous = session([
      event('older-generation-anchor', 20, 'textDelta', '一代历史阅读锚点'),
    ]);
    Object.assign(previous.eventPage, {
      generation: 1,
      coveredRevision: 20,
      newestRevision: 20,
      total: 40,
      hasOlder: true,
      hasNewer: false,
    });
    const compacted = session([
      event('older-generation-crossed', 10, 'textDelta', '不应混入的二代 older 页'),
    ]);
    Object.assign(compacted.eventPage, {
      generation: 2,
      coveredRevision: 10,
      newestRevision: 10,
      total: 20,
      hasOlder: true,
      hasNewer: true,
    });
    vi.mocked(getAcpSession).mockImplementation(async (...args) => (
      typeof args[6]?.beforeSeq === 'number' ? compacted : previous
    ));

    const { container, root } = await renderDialog(previous, 'root', undefined, undefined, locator, 30);
    try {
      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto'));
      expect(scroller).toBeDefined();
      Object.defineProperties(scroller!, {
        clientHeight: { configurable: true, value: 600 },
        scrollHeight: { configurable: true, value: 2_400 },
        scrollTop: { configurable: true, value: 0, writable: true },
      });

      await act(async () => {
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });

      expect(container.textContent).toContain('一代历史阅读锚点');
      expect(container.textContent).not.toContain('不应混入的二代 older 页');
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).not.toBeNull();
    } finally {
      await unmount(root);
    }
  });

  it('atomically replaces the old generation when newer pagination reaches a compacted head', async () => {
    const previous = session([
      event('newer-generation-old', 1, 'textDelta', '一代 newer 交接旧内容'),
    ]);
    Object.assign(previous.eventPage, {
      generation: 1,
      coveredRevision: 1,
      newestRevision: 1,
      total: 2,
      hasOlder: false,
      hasNewer: true,
    });
    const compactedHead = session([
      event('newer-generation-current', 2, 'textDelta', '二代 newer 当前 head'),
    ]);
    Object.assign(compactedHead.eventPage, {
      generation: 2,
      coveredRevision: 2,
      newestRevision: 2,
      total: 2,
      hasOlder: true,
      hasNewer: false,
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(previous)
      .mockResolvedValue(compactedHead);

    const { container, root } = await renderDialog(previous, 'root', undefined, undefined, locator, 30);
    try {
      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto'));
      expect(scroller).toBeDefined();
      Object.defineProperties(scroller!, {
        clientHeight: { configurable: true, value: 600 },
        scrollHeight: { configurable: true, value: 2_400 },
        scrollTop: { configurable: true, value: 1_800, writable: true },
      });

      await act(async () => {
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 300));
      });

      expect(container.textContent).toContain('二代 newer 当前 head');
      expect(container.textContent).not.toContain('一代 newer 交接旧内容');
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
    } finally {
      await unmount(root);
    }
  });

  it('refreshes the canonical head instead of mixing a newer-generation live event into the current window', async () => {
    const previous = session([
      event('live-generation-old', 1, 'textDelta', '一代 live 旧内容'),
    ]);
    const compactedHead = session([
      event('live-generation-current', 2, 'textDelta', '二代 live canonical 内容'),
    ]);
    Object.assign(compactedHead.eventPage, {
      generation: 2,
      coveredRevision: 2,
      newestRevision: 2,
      total: 1,
      hasOlder: true,
      hasNewer: false,
    });
    let resolveCompactedHead!: (value: AcpSessionVm) => void;
    const pendingCompactedHead = new Promise<AcpSessionVm>((resolve) => {
      resolveCompactedHead = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(previous)
      .mockReturnValueOnce(pendingCompactedHead);

    const { container, root } = await renderDialog(previous);
    try {
      await act(async () => {
        runtime.listener?.({
          ...update(event(
            'live-generation-current',
            2,
            'textDelta',
            '二代 live canonical 内容',
          )),
          timelineGeneration: 2,
          timelineRevision: 2,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });

      await vi.waitFor(() => {
        expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
      });
      expect(container.textContent).toContain('一代 live 旧内容');
      expect(container.textContent).not.toContain('二代 live canonical 内容');

      await act(async () => {
        resolveCompactedHead(compactedHead);
        await new Promise((resolve) => window.setTimeout(resolve, 250));
      });

      expect(container.textContent).toContain('二代 live canonical 内容');
      expect(container.textContent).not.toContain('一代 live 旧内容');
      expect(readConversationBranchReplaySnapshot(locator, 'root').events).toEqual([]);
    } finally {
      await unmount(root);
    }
  });

  it('preserves manual scroll when the viewport detaches during an in-flight canonical recovery', async () => {
    const previous = session([
      event('in-flight-scroll-old', 1, 'textDelta', '交接前仍在显示的旧内容'),
    ]);
    const compactedHead = session([
      event('in-flight-scroll-current', 2, 'textDelta', '交接后更新的 canonical 内容'),
    ]);
    Object.assign(compactedHead.eventPage, {
      generation: 2,
      coveredRevision: 2,
      newestRevision: 2,
      total: 1,
      hasOlder: true,
      hasNewer: false,
    });
    let resolveCompactedHead!: (value: AcpSessionVm) => void;
    const pendingCompactedHead = new Promise<AcpSessionVm>((resolve) => {
      resolveCompactedHead = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(previous)
      .mockReturnValueOnce(pendingCompactedHead);

    const { container, root } = await renderDialog(previous);
    try {
      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto'));
      expect(scroller).toBeDefined();
      Object.defineProperties(scroller!, {
        clientHeight: { configurable: true, value: 600 },
        scrollHeight: { configurable: true, value: 2_400 },
        scrollTop: { configurable: true, value: 1_800, writable: true },
      });

      await act(async () => {
        runtime.listener?.({
          ...update(event(
            'in-flight-scroll-current',
            2,
            'textDelta',
            '交接后更新的 canonical 内容',
          )),
          timelineGeneration: 2,
          timelineRevision: 2,
        });
        await vi.waitFor(() => {
          expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
        });
      });

      await act(async () => {
        scroller!.dispatchEvent(new WheelEvent('wheel', {
          bubbles: true,
          deltaY: -1,
        }));
        scroller!.scrollTop = 1_600;
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).not.toBeNull();

      await act(async () => {
        resolveCompactedHead(compactedHead);
        await new Promise((resolve) => window.setTimeout(resolve, 250));
      });

      expect(container.textContent).toContain('交接后更新的 canonical 内容');
      expect(container.textContent).not.toContain('交接前仍在显示的旧内容');
      expect(scroller!.scrollTop).toBe(1_600);
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).not.toBeNull();
    } finally {
      await unmount(root);
    }
  });

  it('keeps one trailing canonical recovery when another malformed event arrives in flight', async () => {
    const previous = session([
      event('malformed-recovery-old', 1, 'textDelta', 'malformed recovery 前的旧内容'),
    ]);
    const firstCanonical = session([
      event('malformed-recovery-answer', 2, 'textDelta', '第一次 recovery 读到的中间内容'),
    ]);
    Object.assign(firstCanonical.eventPage, {
      generation: 2,
      coveredRevision: 2,
      newestRevision: 2,
    });
    const finalCanonical = session([
      event('malformed-recovery-answer', 3, 'textDelta', '第二次 recovery 读到的最终内容', {
        startedSeq: 2,
      }),
    ]);
    Object.assign(finalCanonical.eventPage, {
      generation: 2,
      coveredRevision: 3,
      newestRevision: 3,
    });
    let resolveFirstRecovery!: (value: AcpSessionVm) => void;
    const firstRecovery = new Promise<AcpSessionVm>((resolve) => {
      resolveFirstRecovery = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(previous)
      .mockReturnValueOnce(firstRecovery)
      .mockResolvedValue(finalCanonical);

    const { container, root } = await renderDialog(previous);
    try {
      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          event: event(
            'malformed-recovery-answer',
            2,
            'textDelta',
            '缺失 generation 的第一次 live',
          ),
          timelineRevision: 2,
        });
        await vi.waitFor(() => {
          expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
        });
      });

      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          event: event(
            'malformed-recovery-answer',
            3,
            'textDelta',
            '缺失 generation 的第二次 live',
            { startedSeq: 2 },
          ),
          timelineRevision: 3,
        });
        resolveFirstRecovery(firstCanonical);
        await new Promise((resolve) => window.setTimeout(resolve, 250));
      });

      await vi.waitFor(() => {
        expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(3);
      });
      expect(container.textContent).toContain('第二次 recovery 读到的最终内容');
      expect(container.textContent).not.toContain('第一次 recovery 读到的中间内容');
    } finally {
      await unmount(root);
    }
  });

  it('resumes live projection after an explicit latest handoff consumes a historical recovery gate', async () => {
    const historical = session([
      event('historical-recovery-window', 1, 'textDelta', 'recovery gate 前的历史窗口'),
    ]);
    Object.assign(historical.eventPage, {
      coveredRevision: 1,
      newestRevision: 1,
      hasOlder: true,
      hasNewer: true,
    });
    const canonicalHead = session([
      event('historical-recovery-canonical', 2, 'textDelta', '显式交接后的 canonical head'),
    ]);
    Object.assign(canonicalHead.eventPage, {
      coveredRevision: 2,
      newestRevision: 2,
      hasOlder: true,
      hasNewer: false,
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(historical)
      .mockResolvedValue(canonicalHead);

    const { container, root } = await renderDialog(historical);
    try {
      await detachConversationViewport(container);
      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          event: event(
            'historical-recovery-malformed',
            2,
            'textDelta',
            '缺失 generation 的历史 live',
          ),
          timelineRevision: 2,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(1);

      const returnToLatest = container.querySelector<HTMLButtonElement>(
        '[data-acp-return-to-latest="true"]',
      );
      expect(returnToLatest).not.toBeNull();
      await act(async () => {
        returnToLatest!.click();
        await new Promise((resolve) => window.setTimeout(resolve, 100));
      });
      expect(container.textContent).toContain('显式交接后的 canonical head');

      await act(async () => {
        runtime.listener?.(update(event(
          'historical-recovery-live-after-handoff',
          3,
          'textDelta',
          '交接后继续投影的合法 live',
        )));
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });

      expect(container.textContent).toContain('交接后继续投影的合法 live');
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
    } finally {
      await unmount(root);
    }
  });

  it('queues one recovery handoff when malformed generation arrives during an ordinary latest handoff', async () => {
    const historical = session([
      event('ordinary-handoff-history', 1, 'textDelta', '普通返回最新前的历史窗口'),
    ]);
    Object.assign(historical.eventPage, {
      coveredRevision: 1,
      newestRevision: 1,
      hasOlder: true,
      hasNewer: true,
    });
    const ordinaryHead = session([
      event('ordinary-handoff-head', 2, 'textDelta', '第一次普通交接读到的 head'),
    ]);
    Object.assign(ordinaryHead.eventPage, {
      coveredRevision: 2,
      newestRevision: 2,
      hasOlder: true,
      hasNewer: false,
    });
    const recoveredHead = session([
      event('ordinary-handoff-recovered', 3, 'textDelta', '非法 generation 后重新校准的 head'),
    ]);
    Object.assign(recoveredHead.eventPage, {
      coveredRevision: 3,
      newestRevision: 3,
      hasOlder: true,
      hasNewer: false,
    });
    let resolveOrdinaryHead!: (value: AcpSessionVm) => void;
    const pendingOrdinaryHead = new Promise<AcpSessionVm>((resolve) => {
      resolveOrdinaryHead = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(historical)
      .mockReturnValueOnce(pendingOrdinaryHead)
      .mockResolvedValue(recoveredHead);

    const { container, root } = await renderDialog(historical);
    try {
      await detachConversationViewport(container);
      await act(async () => {
        container.querySelector<HTMLButtonElement>(
          '[data-acp-return-to-latest="true"]',
        )?.click();
        await vi.waitFor(() => {
          expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
        });
      });

      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          event: event(
            'ordinary-handoff-malformed',
            3,
            'textDelta',
            '普通交接期间缺失 generation 的 live',
          ),
          timelineRevision: 3,
        });
        resolveOrdinaryHead(ordinaryHead);
        await new Promise((resolve) => window.setTimeout(resolve, 250));
      });

      await vi.waitFor(() => {
        expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(3);
      });
      expect(container.textContent).toContain('非法 generation 后重新校准的 head');

      await act(async () => {
        runtime.listener?.(update(event(
          'ordinary-handoff-live-after-recovery',
          4,
          'textDelta',
          '普通交接恢复后继续投影的 live',
        )));
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });

      expect(container.textContent).toContain('普通交接恢复后继续投影的 live');
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
    } finally {
      await unmount(root);
    }
  });

  it('drops a delayed live event from an older generation without refreshing the current head', async () => {
    const current = session([
      event('delayed-live-current', 2, 'textDelta', '二代当前 live head'),
    ]);
    current.eventPage.generation = 2;
    current.eventPage.coveredRevision = 2;
    current.eventPage.newestRevision = 2;
    vi.mocked(getAcpSession).mockResolvedValue(current);

    const { container, root } = await renderDialog(current);
    try {
      await act(async () => {
        runtime.listener?.({
          ...update(event(
            'delayed-live-generation-one',
            3,
            'textDelta',
            '不应回写的一代迟到 live',
          )),
          timelineGeneration: 1,
          timelineRevision: 3,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });

      expect(container.textContent).toContain('二代当前 live head');
      expect(container.textContent).not.toContain('不应回写的一代迟到 live');
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(1);
    } finally {
      await unmount(root);
    }
  });

  it('bounds generation-refresh handshakes when replay advances again during the awaited refresh', async () => {
    const stale = session([
      event('bounded-generation-old', 1, 'textDelta', '有界刷新前的一代内容'),
    ]);
    const generationTwo = session([
      event('bounded-generation-two', 2, 'textDelta', '有界刷新拿到的二代内容'),
    ]);
    generationTwo.eventPage.generation = 2;
    generationTwo.eventPage.coveredRevision = 2;
    const generationThree = event(
      'bounded-generation-three',
      3,
      'textDelta',
      '等待期间到达的三代 replay',
    );
    let resolveGenerationTwo!: (value: AcpSessionVm) => void;
    const pendingGenerationTwo = new Promise<AcpSessionVm>((resolve) => {
      resolveGenerationTwo = resolve;
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(stale)
      .mockReturnValueOnce(pendingGenerationTwo)
      .mockResolvedValue(generationTwo);
    applyConversationEventToBranchSnapshots({
      ...update(event('bounded-generation-two-replay', 2, 'textDelta', '二代 replay')),
      timelineGeneration: 2,
      timelineRevision: 2,
    });

    const { container, root } = await renderDialog(stale);
    try {
      await vi.waitFor(() => {
        expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
      });
      applyConversationEventToBranchSnapshots({
        ...update(generationThree),
        timelineGeneration: 3,
        timelineRevision: 3,
      });
      await act(async () => {
        resolveGenerationTwo(generationTwo);
        await new Promise((resolve) => window.setTimeout(resolve, 80));
      });

      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(2);
      expect(container.textContent).not.toContain('等待期间到达的三代 replay');
    } finally {
      await unmount(root);
    }
  });

  it('hands off once instead of projecting an overflowed paused live buffer', async () => {
    const pageSize = 22;
    const pendingCapacity = loadedEventBufferLimit(pageSize);
    const initial = session([
      event('paused-overflow-initial', 1, 'textDelta', '暂停前的 canonical 内容'),
    ]);
    const canonicalHead = session([
      event(
        'paused-overflow-canonical',
        pendingCapacity + 2,
        'textDelta',
        '恢复后的 canonical head',
      ),
    ]);
    Object.assign(canonicalHead.eventPage, {
      coveredRevision: pendingCapacity + 2,
      newestRevision: pendingCapacity + 2,
      total: pendingCapacity + 2,
      hasOlder: true,
      hasNewer: false,
    });
    let canonicalReadCount = 0;
    vi.mocked(getAcpSession).mockImplementation(async (...args) => {
      const options = args[6];
      if (options?.afterRevision != null) return canonicalHead;
      canonicalReadCount += 1;
      return canonicalReadCount === 1 ? initial : canonicalHead;
    });

    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const renderPausedState = async (paused: boolean) => {
      await act(async () => {
        root.render(
          <TooltipProvider>
            <ACPChatDialog
              session={initial}
              {...locator}
              branchId="root"
              eventPageSize={pageSize}
              liveUpdatesPaused={paused}
              showSystemPromptAction={false}
              showRawFramesAction={false}
              usageCompact
            />
          </TooltipProvider>,
        );
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
    };

    try {
      expect(pendingCapacity).toBeGreaterThan(64);
      await renderPausedState(true);
      await vi.waitFor(() => expect(canonicalReadCount).toBe(1));

      await act(async () => {
        for (let index = 0; index <= pendingCapacity; index += 1) {
          runtime.listener?.(update(event(
            `paused-overflow-${index}`,
            index + 2,
            'textDelta',
            `不应逐项投影的 paused live ${index}`,
          )));
        }
        await new Promise((resolve) => window.setTimeout(resolve, 30));
      });

      expect(container.textContent).toContain('暂停前的 canonical 内容');
      expect(container.textContent).not.toContain('不应逐项投影的 paused live');

      await renderPausedState(false);
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 250));
      });

      expect({
        canonicalReadCount,
        canonicalVisible: container.textContent?.includes('恢复后的 canonical head'),
        bufferedProjectionVisible: container.textContent?.includes('不应逐项投影的 paused live'),
      }).toEqual({
        canonicalReadCount: 2,
        canonicalVisible: true,
        bufferedProjectionVisible: false,
      });
    } finally {
      await unmount(root);
    }
  });

  it('discards a scheduled coalesced live frame when pause begins before the 125ms drain', async () => {
    const initialEvent = event(
      'pause-transition-initial',
      1,
      'textDelta',
      '暂停过渡前的 canonical 内容',
    );
    const initial = session([initialEvent]);
    const canonicalHead = session([
      initialEvent,
      event(
        'pause-transition-answer',
        3,
        'textDelta',
        '解除暂停后的最终 canonical head',
        { startedSeq: 2 },
      ),
    ]);
    Object.assign(canonicalHead.eventPage, {
      coveredRevision: 3,
      newestRevision: 3,
      total: 2,
      hasOlder: false,
      hasNewer: false,
    });
    let canonicalReadCount = 0;
    vi.mocked(getAcpSession).mockImplementation(async (...args) => {
      if (args[6]?.afterRevision != null) return canonicalHead;
      canonicalReadCount += 1;
      return canonicalReadCount === 1 ? initial : canonicalHead;
    });

    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const renderPausedState = async (paused: boolean) => {
      await act(async () => {
        root.render(
          <TooltipProvider>
            <ACPChatDialog
              session={initial}
              {...locator}
              branchId="root"
              liveUpdatesPaused={paused}
              showSystemPromptAction={false}
              showRawFramesAction={false}
              usageCompact
            />
          </TooltipProvider>,
        );
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
    };

    try {
      await renderPausedState(false);
      await vi.waitFor(() => expect(canonicalReadCount).toBe(1));

      await act(async () => {
        runtime.listener?.(update(event(
          'pause-transition-answer',
          2,
          'textDelta',
          '不应 drain 到 DOM 的 coalesced live',
        )));
        await Promise.resolve();
      });
      expect(container.textContent).not.toContain('不应 drain 到 DOM 的 coalesced live');

      await renderPausedState(true);
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });
      expect({
        canonicalReadCount,
        bufferedProjectionVisible: container.textContent?.includes(
          '不应 drain 到 DOM 的 coalesced live',
        ),
      }).toEqual({
        canonicalReadCount: 1,
        bufferedProjectionVisible: false,
      });

      await renderPausedState(false);
      await vi.waitFor(() => expect(canonicalReadCount).toBe(2));
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });

      expect({
        canonicalReadCount,
        canonicalVisible: container.textContent?.includes(
          '解除暂停后的最终 canonical head',
        ),
        bufferedProjectionVisible: container.textContent?.includes(
          '不应 drain 到 DOM 的 coalesced live',
        ),
      }).toEqual({
        canonicalReadCount: 2,
        canonicalVisible: true,
        bufferedProjectionVisible: false,
      });
    } finally {
      await unmount(root);
    }
  });

  it('keeps a paused full-session body out of the timeline while applying lifecycle control', async () => {
    const turnId = 'pause-session-envelope-turn';
    const initialLifecycle: ConversationAttemptLifecycleVm = {
      ...terminalLifecycle(turnId),
      runtime: {
        status: 'running',
        outcome: null,
        pauseReason: null,
        resumable: false,
        current: true,
        active: true,
        continuable: false,
        phase: 'provider-running',
        revision: 6,
      },
      acp: {
        revision: 10,
        turnId,
        sessionAvailability: 'established',
        liveTurnActivity: 'running',
        latestTurnStatus: 'none',
        stopping: false,
      },
      displayStatus: 'running',
      runtimeDisplay: {
        code: 'running',
        tone: 'running',
        icon: 'dot',
        terminal: false,
        resumable: false,
        reasonCode: null,
        blockingError: false,
      },
      composer: {
        mode: 'runtime-active',
        submitTarget: 'none',
        processingKind: 'processing',
        statusKey: 'conversation.runtime.runtimeActive',
        canStop: true,
        lockInput: true,
      },
    };
    const completedLifecycle = terminalLifecycle(turnId);
    const initialEvent = event(
      'pause-session-envelope-initial',
      1,
      'textDelta',
      '暂停 session envelope 前的可见内容',
    );
    const initial = session([initialEvent]);
    const pausedEnvelope = session([
      initialEvent,
      event(
        'pause-session-envelope-answer',
        2,
        'textDelta',
        '暂停期间不应投影的 full session body',
      ),
    ]);
    const canonicalHead = session([
      initialEvent,
      event(
        'pause-session-envelope-answer',
        3,
        'textDelta',
        '解除暂停后读取的 session canonical head',
        { startedSeq: 2 },
      ),
    ], 'paused');
    Object.assign(pausedEnvelope.eventPage, {
      coveredRevision: 2,
      newestRevision: 2,
      total: 2,
    });
    Object.assign(canonicalHead.eventPage, {
      coveredRevision: 3,
      newestRevision: 3,
      total: 2,
      hasOlder: false,
      hasNewer: false,
    });
    let canonicalReadCount = 0;
    vi.mocked(getAcpSession).mockImplementation(async (...args) => {
      if (args[6]?.afterRevision != null) return canonicalHead;
      canonicalReadCount += 1;
      return canonicalReadCount === 1 ? initial : canonicalHead;
    });

    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const renderPausedState = async (paused: boolean) => {
      await act(async () => {
        root.render(
          <TooltipProvider>
            <ACPChatDialog
              session={initial}
              {...locator}
              branchId="root"
              liveUpdatesPaused={paused}
              runtimeComposerContext={{
                isOrchestrated: true,
                runtimeStatus: initialLifecycle.runtime.status,
                workflowValid: true,
                lifecycle: initialLifecycle,
              }}
              showSystemPromptAction={false}
              showRawFramesAction={false}
              usageCompact
            />
          </TooltipProvider>,
        );
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
    };

    try {
      await renderPausedState(true);
      await vi.waitFor(() => expect(canonicalReadCount).toBe(1));
      expect(container.querySelector<HTMLTextAreaElement>('textarea')?.disabled).toBe(true);

      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          timelineGeneration: 1,
          timelineRevision: 2,
          session: pausedEnvelope,
          lifecycle: completedLifecycle,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      expect({
        lifecycleControlApplied: container.querySelector<HTMLTextAreaElement>('textarea')?.disabled,
        initialTimelineVisible: container.textContent?.includes(
          '暂停 session envelope 前的可见内容',
        ),
        pausedBodyVisible: container.textContent?.includes(
          '暂停期间不应投影的 full session body',
        ),
      }).toEqual({
        lifecycleControlApplied: false,
        initialTimelineVisible: true,
        pausedBodyVisible: false,
      });

      await renderPausedState(false);
      await vi.waitFor(() => expect(canonicalReadCount).toBe(2));
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });

      expect({
        canonicalReadCount,
        canonicalVisible: container.textContent?.includes(
          '解除暂停后读取的 session canonical head',
        ),
        pausedBodyVisible: container.textContent?.includes(
          '暂停期间不应投影的 full session body',
        ),
      }).toEqual({
        canonicalReadCount: 2,
        canonicalVisible: true,
        pausedBodyVisible: false,
      });
    } finally {
      await unmount(root);
    }
  });

  it('invalidates an older cursor after pagination crosses generation but keeps explicit latest recovery', async () => {
    const initial = session([
      event('older-cursor-generation-one', 20, 'textDelta', '一代历史窗口'),
    ]);
    Object.assign(initial.eventPage, {
      generation: 1,
      coveredRevision: 20,
      newestRevision: 20,
      hasOlder: true,
      hasNewer: false,
    });
    const crossedGeneration = session([
      event('older-cursor-generation-two', 10, 'textDelta', '二代旧页不应混入'),
    ]);
    Object.assign(crossedGeneration.eventPage, {
      generation: 2,
      coveredRevision: 10,
      newestRevision: 10,
      hasOlder: true,
      hasNewer: true,
    });
    const canonicalHead = session([
      event('older-cursor-canonical-head', 30, 'textDelta', '二代 canonical head'),
    ]);
    Object.assign(canonicalHead.eventPage, {
      generation: 2,
      coveredRevision: 30,
      newestRevision: 30,
      hasOlder: true,
      hasNewer: false,
    });
    let plainReadCount = 0;
    let olderReadCount = 0;
    vi.mocked(getAcpSession).mockImplementation(async (...args) => {
      const options = args[6];
      if (options?.beforeSeq != null) {
        olderReadCount += 1;
        return crossedGeneration;
      }
      plainReadCount += 1;
      return plainReadCount === 1 ? initial : canonicalHead;
    });

    const { container, root } = await renderDialog(initial, 'root', undefined, undefined, locator, 30);
    try {
      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto'));
      expect(scroller).toBeDefined();
      Object.defineProperties(scroller!, {
        clientHeight: { configurable: true, value: 600 },
        scrollHeight: { configurable: true, value: 2_400 },
        scrollTop: { configurable: true, value: 0, writable: true },
      });

      await act(async () => {
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 30));
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 30));
      });

      const returnToLatest = container.querySelector<HTMLButtonElement>(
        '[data-acp-return-to-latest="true"]',
      );
      expect(returnToLatest).not.toBeNull();
      await act(async () => {
        returnToLatest!.click();
        await new Promise((resolve) => window.setTimeout(resolve, 100));
      });

      expect({
        olderReadCount,
        plainReadCount,
        canonicalVisible: container.textContent?.includes('二代 canonical head'),
        crossedPageVisible: container.textContent?.includes('二代旧页不应混入'),
      }).toEqual({
        olderReadCount: 1,
        plainReadCount: 2,
        canonicalVisible: true,
        crossedPageVisible: false,
      });
    } finally {
      await unmount(root);
    }
  });

  it('invalidates a newer cursor after pagination crosses generation but keeps explicit latest recovery', async () => {
    const initial = session([
      event('newer-cursor-generation-one', 20, 'textDelta', '一代较旧窗口'),
    ]);
    Object.assign(initial.eventPage, {
      generation: 1,
      coveredRevision: 40,
      newestRevision: 20,
      hasOlder: true,
      hasNewer: true,
    });
    const crossedGeneration = session([
      event('newer-cursor-generation-two', 30, 'textDelta', '二代中间页不应混入'),
    ]);
    Object.assign(crossedGeneration.eventPage, {
      generation: 2,
      coveredRevision: 50,
      newestRevision: 30,
      hasOlder: true,
      hasNewer: true,
    });
    const canonicalHead = session([
      event('newer-cursor-canonical-head', 50, 'textDelta', '二代最新 canonical head'),
    ]);
    Object.assign(canonicalHead.eventPage, {
      generation: 2,
      coveredRevision: 50,
      newestRevision: 50,
      hasOlder: true,
      hasNewer: false,
    });
    let plainReadCount = 0;
    let newerReadCount = 0;
    vi.mocked(getAcpSession).mockImplementation(async (...args) => {
      const options = args[6];
      if (options?.afterSeq != null) {
        newerReadCount += 1;
        return crossedGeneration;
      }
      plainReadCount += 1;
      return plainReadCount === 1 ? initial : canonicalHead;
    });

    const { container, root } = await renderDialog(initial, 'root', undefined, undefined, locator, 30);
    try {
      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto'));
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
      await act(async () => {
        scroller!.scrollTop = 1_600;
        scroller!.dispatchEvent(new WheelEvent('wheel', {
          bubbles: true,
          deltaY: -100,
        }));
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 30));
      });

      const returnToLatest = container.querySelector<HTMLButtonElement>(
        '[data-acp-return-to-latest="true"]',
      );
      expect(returnToLatest).not.toBeNull();
      await act(async () => {
        returnToLatest!.click();
        await new Promise((resolve) => window.setTimeout(resolve, 100));
      });

      expect({
        newerReadCount,
        plainReadCount,
        canonicalVisible: container.textContent?.includes('二代最新 canonical head'),
        crossedPageVisible: container.textContent?.includes('二代中间页不应混入'),
      }).toEqual({
        newerReadCount: 1,
        plainReadCount: 2,
        canonicalVisible: true,
        crossedPageVisible: false,
      });
    } finally {
      await unmount(root);
    }
  });

  it('stops replay-loss catch-up after four fixed-cut pages and keeps recovery pending', async () => {
    const initial = session([
      event('bounded-catch-up-initial', 1, 'textDelta', '缺口前的 snapshot'),
    ]);
    initial.eventPage.coveredRevision = 1;
    initial.eventPage.newestRevision = 1;
    const requestedAfterRevisions: number[] = [];
    vi.mocked(getAcpSession).mockImplementation(async (...args) => {
      const afterRevision = args[6]?.afterRevision;
      if (afterRevision == null) return initial;
      requestedAfterRevisions.push(afterRevision);
      const nextRevision = afterRevision + 1;
      const delta = session([
        event(
          `bounded-catch-up-${nextRevision}`,
          nextRevision,
          'textDelta',
          `追平到 revision ${nextRevision}`,
        ),
      ]);
      delta.eventPage.coveredRevision = nextRevision;
      delta.eventPage.newestRevision = nextRevision;
      return delta;
    });
    applyConversationEventToBranchSnapshots(update(event(
      'bounded-catch-up-loss',
      10,
      'textDelta',
      '无法保留的大事件',
      { raw: { oversized: 'x'.repeat(CONVERSATION_EVENT_REPLAY_LIMITS.eventBytes) } },
    )));

    const { root } = await renderDialog(initial);
    try {
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 120));
      });

      const replay = readConversationBranchReplaySnapshot(locator, 'root');
      expect({
        requestedAfterRevisions,
        requiresCatchUp: replay.requiresCatchUp,
        lossWatermarkRevision: replay.lossWatermarkRevision,
      }).toEqual({
        requestedAfterRevisions: [1, 2, 3, 4],
        requiresCatchUp: true,
        lossWatermarkRevision: 10,
      });
    } finally {
      await unmount(root);
    }
  });

  it('catches up only the fixed C0 replay cut when C1 advances during I/O', async () => {
    const initial = session([
      event('fixed-cut-initial', 1, 'textDelta', '固定切片前的 snapshot'),
    ]);
    initial.eventPage.coveredRevision = 1;
    initial.eventPage.newestRevision = 1;
    const requestedAfterRevisions: number[] = [];
    let advancedC1 = false;
    vi.mocked(getAcpSession).mockImplementation(async (...args) => {
      const afterRevision = args[6]?.afterRevision;
      if (afterRevision == null) return initial;
      requestedAfterRevisions.push(afterRevision);
      if (!advancedC1) {
        advancedC1 = true;
        applyConversationEventToBranchSnapshots(update(event(
          'fixed-cut-c1-loss',
          10,
          'textDelta',
          '追平期间抵达的新缺口',
          { raw: { oversized: 'y'.repeat(CONVERSATION_EVENT_REPLAY_LIMITS.eventBytes) } },
        )));
      }
      const nextRevision = afterRevision + 1;
      const delta = session([
        event(
          `fixed-cut-delta-${nextRevision}`,
          nextRevision,
          'textDelta',
          `C0 追平到 revision ${nextRevision}`,
        ),
      ]);
      delta.eventPage.coveredRevision = nextRevision;
      delta.eventPage.newestRevision = nextRevision;
      return delta;
    });
    applyConversationEventToBranchSnapshots(update(event(
      'fixed-cut-c0-loss',
      3,
      'textDelta',
      'C0 缺口',
      { raw: { oversized: 'x'.repeat(CONVERSATION_EVENT_REPLAY_LIMITS.eventBytes) } },
    )));

    const { root } = await renderDialog(initial);
    try {
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 120));
      });

      const replay = readConversationBranchReplaySnapshot(locator, 'root');
      expect({
        requestedAfterRevisions,
        requiresCatchUp: replay.requiresCatchUp,
        lossWatermarkRevision: replay.lossWatermarkRevision,
      }).toEqual({
        requestedAfterRevisions: [1, 2],
        requiresCatchUp: true,
        lossWatermarkRevision: 10,
      });
    } finally {
      await unmount(root);
    }
  });

  it('settles a historical recovery gate when newer pagination naturally reaches the head', async () => {
    const historical = session([
      event('newer-edge-recovery-history', 1, 'textDelta', '自然分页前的历史窗口'),
    ]);
    Object.assign(historical.eventPage, {
      generation: 1,
      coveredRevision: 1,
      newestRevision: 1,
      total: 2,
      hasOlder: false,
      hasNewer: true,
    });
    const canonicalHead = session([
      event('newer-edge-recovery-head', 2, 'textDelta', '自然分页抵达的 canonical head'),
    ]);
    Object.assign(canonicalHead.eventPage, {
      generation: 1,
      coveredRevision: 2,
      newestRevision: 2,
      total: 2,
      hasOlder: true,
      hasNewer: false,
    });
    vi.mocked(getAcpSession)
      .mockResolvedValueOnce(historical)
      .mockResolvedValue(canonicalHead);

    const { container, root } = await renderDialog(
      historical,
      'root',
      undefined,
      undefined,
      locator,
      30,
    );
    try {
      await act(async () => {
        runtime.listener?.({
          ...locator,
          branchId: 'root',
          event: event(
            'newer-edge-recovery-malformed',
            2,
            'textDelta',
            '缺失 generation 的历史 live',
          ),
          timelineRevision: 2,
        });
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(1);
      await detachConversationViewport(container);
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).not.toBeNull();

      const scroller = [...container.querySelectorAll<HTMLDivElement>('div')]
        .find((element) => element.classList.contains('h-full')
          && element.classList.contains('overflow-y-auto'));
      expect(scroller).toBeDefined();
      Object.defineProperties(scroller!, {
        clientHeight: { configurable: true, value: 600 },
        scrollHeight: { configurable: true, value: 2_400 },
        scrollTop: { configurable: true, value: 1_800, writable: true },
      });

      await act(async () => {
        scroller!.dispatchEvent(new WheelEvent('wheel', {
          bubbles: true,
          deltaY: 100,
        }));
        scroller!.dispatchEvent(new Event('scroll'));
        await new Promise((resolve) => window.setTimeout(resolve, 300));
      });

      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(3);
      expect(vi.mocked(getAcpSession).mock.calls[1]?.[6]).toMatchObject({
        afterSeq: 1,
      });
      expect(vi.mocked(getAcpSession).mock.calls[2]?.[6]).not.toHaveProperty('afterSeq');
      expect(vi.mocked(getAcpSession).mock.calls[2]?.[6]).not.toHaveProperty('beforeSeq');
      expect(container.textContent).toContain('自然分页抵达的 canonical head');
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).not.toBeNull();

      await act(async () => {
        scroller!.dispatchEvent(new Event('scrollend'));
        await new Promise((resolve) => window.setTimeout(resolve, 50));
      });
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();

      await act(async () => {
        runtime.listener?.(update(event(
          'newer-edge-recovery-live-after-head',
          3,
          'textDelta',
          '自然分页后继续投影的合法 live',
        )));
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });

      expect(container.textContent).toContain('自然分页后继续投影的合法 live');
      expect(container.querySelector('[data-acp-return-to-latest="true"]')).toBeNull();
      expect(vi.mocked(getAcpSession)).toHaveBeenCalledTimes(3);
    } finally {
      await unmount(root);
    }
  });
});
