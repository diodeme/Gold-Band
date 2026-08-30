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
