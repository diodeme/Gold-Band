import { afterEach, describe, expect, it, vi } from 'vitest';

const { nativeState, nativeSubscription } = vi.hoisted(() => ({
  nativeState: {
    listener: null as null | ((event: unknown) => void),
  },
  nativeSubscription: vi.fn(),
}));

vi.mock('@/api/client', () => ({
  getRuntimeApi: () => ({
    subscribeAcpSessionUpdates: nativeSubscription,
  }),
}));

import type { AcpSessionUpdatedEventVm } from '@/api/client';
import {
  ensureConversationEventRouterStarted,
  readConversationBranchReplaySnapshot,
  resetConversationEventRouterSnapshots,
  subscribeConversationAttemptEvents,
  subscribeConversationEvents,
} from '@/lib/conversation-event-router';
import type { AcpUiEventVm } from '@/types';

const baseLocator = {
  projectId: 'project-router',
  taskId: 'task-router',
  taskUuid: 'task-uuid-router',
  runId: 'run-router',
  roundId: 'round-router',
  nodeId: 'node-router',
  attemptId: 'attempt-router',
};

function uiEvent(id: string): AcpUiEventVm {
  return {
    id,
    seq: 1,
    timestamp: '1Z',
    kind: 'textDelta',
    sessionId: 'session-router',
    content: id,
    title: null,
    toolCallId: null,
    status: null,
    raw: null,
  };
}

function publish(event: AcpSessionUpdatedEventVm) {
  expect(nativeState.listener).not.toBeNull();
  nativeState.listener?.(event);
}

afterEach(() => {
  resetConversationEventRouterSnapshots();
  nativeState.listener = null;
  vi.clearAllMocks();
});

describe('conversation event router subscriptions', () => {
  it('retries a failed native subscription for existing listeners without duplicating the stream', async () => {
    vi.useFakeTimers();
    vi.resetModules();
    nativeState.listener = null;
    const subscribeError = new Error('native listen unavailable');
    const nativeUnlisten = vi.fn();
    nativeSubscription
      .mockRejectedValueOnce(subscribeError)
      .mockImplementationOnce(async (listener: (event: unknown) => void) => {
        nativeState.listener = listener;
        return nativeUnlisten;
      });
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    const router = await import('@/lib/conversation-event-router');
    const listener = vi.fn();
    const dispose = router.subscribeConversationAttemptEvents(baseLocator, listener);

    try {
      await vi.advanceTimersByTimeAsync(0);
      expect(nativeSubscription).toHaveBeenCalledTimes(1);

      await vi.runOnlyPendingTimersAsync();
      expect(nativeSubscription).toHaveBeenCalledTimes(2);

      publish({
        ...baseLocator,
        branchId: 'root',
        event: uiEvent('recovered-listener-event'),
        timelineGeneration: 1,
        timelineRevision: 1,
      });
      expect(listener).toHaveBeenCalledOnce();
      expect(consoleError).toHaveBeenCalledWith(
        '[Gold Band] Conversation event router subscription failed',
        expect.objectContaining({ error: subscribeError }),
      );

      await vi.runOnlyPendingTimersAsync();
      expect(nativeSubscription).toHaveBeenCalledTimes(2);
      expect(nativeUnlisten).not.toHaveBeenCalled();
    } finally {
      dispose();
      router.resetConversationEventRouterSnapshots();
      consoleError.mockRestore();
      vi.useRealTimers();
    }
  });

  it('cancels a pending subscription retry when the last listener leaves', async () => {
    vi.useFakeTimers();
    vi.resetModules();
    nativeState.listener = null;
    const nativeUnlisten = vi.fn();
    nativeSubscription
      .mockRejectedValueOnce(new Error('native listen unavailable'))
      .mockImplementationOnce(async (listener: (event: unknown) => void) => {
        nativeState.listener = listener;
        return nativeUnlisten;
      });
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    const router = await import('@/lib/conversation-event-router');
    const firstDispose = router.subscribeConversationEvents(vi.fn());
    let secondDispose: (() => void) | null = null;

    try {
      await vi.advanceTimersByTimeAsync(0);
      expect(nativeSubscription).toHaveBeenCalledTimes(1);
      expect(vi.getTimerCount()).toBe(1);

      firstDispose();
      expect(vi.getTimerCount()).toBe(0);
      await vi.runOnlyPendingTimersAsync();
      expect(nativeSubscription).toHaveBeenCalledTimes(1);

      secondDispose = router.subscribeConversationEvents(vi.fn());
      await vi.advanceTimersByTimeAsync(0);
      expect(nativeSubscription).toHaveBeenCalledTimes(2);
      expect(nativeState.listener).not.toBeNull();
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      secondDispose?.();
      router.resetConversationEventRouterSnapshots();
      consoleError.mockRestore();
      vi.useRealTimers();
    }
  });

  it('removes an invalid-generation event before keyed and global delivery while preserving controls', async () => {
    vi.resetModules();
    nativeState.listener = null;
    nativeSubscription.mockImplementation(async (listener: (event: unknown) => void) => {
      nativeState.listener = listener;
      return vi.fn();
    });
    const router = await import('@/lib/conversation-event-router');
    const session: NonNullable<AcpSessionUpdatedEventVm['session']> = {
      branchId: 'root',
      parentBranchId: null,
      readOnly: false,
      sessionId: 'session-router',
      provider: 'test',
      status: 'running',
      restored: false,
      events: [],
      eventPage: {
        generation: 1,
        coveredRevision: 1,
        loadedCount: 0,
        total: 0,
        hasOlder: false,
        hasNewer: false,
      },
      timelineProjection: { agents: [], todoEntries: [] },
      pendingInteractions: [],
      diagnostics: { rawFrameCount: 0, eventCount: 0, errorCount: 0 },
    };
    const lifecycle: NonNullable<AcpSessionUpdatedEventVm['lifecycle']> = {
      runtime: {
        status: 'paused',
        outcome: null,
        pauseReason: null,
        resumable: true,
        current: true,
        active: false,
        continuable: true,
        phase: 'idle',
        revision: 1,
      },
      control: { mode: 'non-runtime-controlled' },
      acp: {
        revision: 1,
        sessionAvailability: 'established',
        liveTurnActivity: 'running',
        latestTurnStatus: 'none',
        stopping: false,
      },
      displayStatus: 'running',
      runtimeDisplay: {
        code: 'paused',
        tone: 'warning',
        icon: 'pause',
        terminal: false,
        resumable: true,
        blockingError: false,
      },
      composer: {
        mode: 'normal',
        submitTarget: 'acp-prompt',
        processingKind: 'responding',
        canStop: true,
        lockInput: false,
      },
    };
    const malformed = {
      ...baseLocator,
      branchId: 'root',
      event: uiEvent('generationless-event'),
      timelineRevision: 1,
      session,
      lifecycle,
    } as unknown as AcpSessionUpdatedEventVm;
    const attemptListener = vi.fn();
    const globalListener = vi.fn();
    const disposers = [
      router.subscribeConversationAttemptEvents(baseLocator, attemptListener),
      router.subscribeConversationEvents(globalListener),
    ];

    try {
      await router.ensureConversationEventRouterStarted();
      publish(malformed);

      expect(attemptListener).toHaveBeenCalledOnce();
      expect(globalListener).toHaveBeenCalledOnce();
      for (const listener of [attemptListener, globalListener]) {
        const delivered = listener.mock.calls[0]?.[0] as AcpSessionUpdatedEventVm;
        expect(delivered.event ?? null).toBeNull();
        expect(delivered.timelineRecoveryRequired).toBe(true);
        expect(delivered.session).toBe(session);
        expect(delivered.lifecycle).toBe(lifecycle);
      }
    } finally {
      disposers.forEach((dispose) => dispose());
      router.resetConversationEventRouterSnapshots();
    }
  });

  it('owns one native stream, writes replay first, and visits only the matching attempt listeners', async () => {
    nativeSubscription.mockImplementation(async (listener: (event: unknown) => void) => {
      nativeState.listener = listener;
      return vi.fn();
    });
    const globalListener = vi.fn();
    const matchingListener = vi.fn(() => {
      expect(readConversationBranchReplaySnapshot(baseLocator, 'root').events)
        .toEqual([expect.objectContaining({ id: 'target-event' })]);
    });
    const otherListener = vi.fn();
    const disposers = [
      subscribeConversationEvents(globalListener),
      subscribeConversationAttemptEvents(baseLocator, matchingListener),
      subscribeConversationAttemptEvents(
        { ...baseLocator, taskUuid: 'other-task-uuid' },
        otherListener,
      ),
    ];
    const unrelatedListeners = Array.from({ length: 128 }, () => vi.fn());
    unrelatedListeners.forEach((listener, index) => {
      disposers.push(subscribeConversationAttemptEvents(
        { ...baseLocator, taskUuid: `unrelated-task-${index}` },
        listener,
      ));
    });

    try {
      await ensureConversationEventRouterStarted();
      expect(nativeSubscription).toHaveBeenCalledTimes(1);

      publish({
        ...baseLocator,
        branchId: 'root',
        event: uiEvent('target-event'),
        timelineGeneration: 1,
        timelineRevision: 1,
      });

      expect(matchingListener).toHaveBeenCalledTimes(1);
      expect(globalListener).toHaveBeenCalledTimes(1);
      expect(otherListener).not.toHaveBeenCalled();
      expect(unrelatedListeners.every((listener) => listener.mock.calls.length === 0)).toBe(true);

      publish({
        ...baseLocator,
        taskId: 'background-task',
        taskUuid: 'background-task-uuid',
        branchId: 'agent-background',
        event: uiEvent('background-event'),
        timelineGeneration: 1,
        timelineRevision: 1,
      });
      expect(readConversationBranchReplaySnapshot(
        { ...baseLocator, taskId: 'background-task', taskUuid: 'background-task-uuid' },
        'agent-background',
      ).events).toEqual([expect.objectContaining({ id: 'background-event' })]);
    } finally {
      disposers.forEach((dispose) => dispose());
    }
  });

  it('isolates a throwing attempt listener from later attempt and global listeners', async () => {
    vi.resetModules();
    nativeState.listener = null;
    nativeSubscription.mockImplementation(async (listener: (event: unknown) => void) => {
      nativeState.listener = listener;
      return vi.fn();
    });
    const router = await import('@/lib/conversation-event-router');
    const listenerError = new Error('attempt listener failed');
    const survivingAttemptListener = vi.fn();
    const globalListener = vi.fn();
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    const disposers = [
      router.subscribeConversationAttemptEvents(baseLocator, () => {
        throw listenerError;
      }),
      router.subscribeConversationAttemptEvents(baseLocator, survivingAttemptListener),
      router.subscribeConversationEvents(globalListener),
    ];

    try {
      await router.ensureConversationEventRouterStarted();
      const event: AcpSessionUpdatedEventVm = {
        ...baseLocator,
        branchId: 'root',
        event: uiEvent('listener-isolation-event'),
        timelineGeneration: 1,
        timelineRevision: 1,
      };

      expect(() => publish(event)).not.toThrow();
      expect(survivingAttemptListener).toHaveBeenCalledOnce();
      expect(survivingAttemptListener).toHaveBeenCalledWith(event);
      expect(globalListener).toHaveBeenCalledOnce();
      expect(globalListener).toHaveBeenCalledWith(event);
      expect(consoleError).toHaveBeenCalledWith(
        '[Gold Band] Conversation event listener failed',
        expect.objectContaining({
          scope: 'attempt',
          error: listenerError,
        }),
      );
    } finally {
      disposers.forEach((dispose) => dispose());
      consoleError.mockRestore();
      router.resetConversationEventRouterSnapshots();
    }
  });
});
