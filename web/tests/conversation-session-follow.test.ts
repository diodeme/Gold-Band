import { describe, expect, it } from 'vitest';
import {
  areDynamicConversationSiblingSessionKeys,
  conversationAcpRunRefreshStatus,
  planConversationAcpRunUpdate,
  resolveConversationEventSelectedSessionKey,
  resolveConversationRefreshSelectedSessionKey,
  resolveConversationRunReentrySelection,
  shouldEnableConversationAutoFollow,
  shouldQueueConversationRunRefreshForAcpUpdate,
} from '@/lib/conversation-session-follow';

function runPageResetCount(runIds: string[]) {
  let previousRunId: string | null = null;
  let resets = 0;
  for (const runId of runIds) {
    if (runId !== previousRunId) {
      resets += 1;
      previousRunId = runId;
    }
  }
  return resets;
}

describe('conversation session follow helpers', () => {
  const availableSessionKeys = new Set([
    'round-001/history/attempt-001',
    'round-001/latest/attempt-002',
  ]);

  it('restores a remembered manual attempt instead of the latest attempt on reentry', () => {
    expect(resolveConversationRunReentrySelection({
      followMode: 'manual',
      rememberedSelectedSessionKey: 'round-001/history/attempt-001',
      defaultSelectedSessionKey: 'round-001/latest/attempt-002',
      hasSessionKey: (key) => availableSessionKeys.has(key),
    })).toEqual({
      followMode: 'manual',
      selectedSessionKey: 'round-001/history/attempt-001',
      preserveSelectedSession: true,
    });
  });

  it('lets an explicit attempt deep link override remembered navigation state', () => {
    expect(resolveConversationRunReentrySelection({
      followMode: 'auto',
      rememberedSelectedSessionKey: 'round-001/latest/attempt-002',
      explicitSelectedSessionKey: 'round-001/history/attempt-001',
      defaultSelectedSessionKey: 'round-001/latest/attempt-002',
      hasSessionKey: (key) => availableSessionKeys.has(key),
    })).toEqual({
      followMode: 'manual',
      selectedSessionKey: 'round-001/history/attempt-001',
      preserveSelectedSession: true,
    });
  });

  it('falls back deterministically without re-enabling auto-follow when a remembered attempt is unavailable', () => {
    expect(resolveConversationRunReentrySelection({
      followMode: 'manual',
      rememberedSelectedSessionKey: 'round-001/missing/attempt-001',
      defaultSelectedSessionKey: 'round-001/latest/attempt-002',
      hasSessionKey: (key) => availableSessionKeys.has(key),
    })).toEqual({
      followMode: 'manual',
      selectedSessionKey: 'round-001/latest/attempt-002',
      preserveSelectedSession: true,
    });
  });

  it('uses the latest backend selection when auto-follow reenters without a user selection change', () => {
    expect(resolveConversationRunReentrySelection({
      followMode: 'auto',
      defaultSelectedSessionKey: 'round-001/latest/attempt-002',
      hasSessionKey: (key) => availableSessionKeys.has(key),
    })).toEqual({
      followMode: 'auto',
      selectedSessionKey: 'round-001/latest/attempt-002',
      preserveSelectedSession: false,
    });
  });

  it('selects the incoming session when there is no current selection', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: null,
      incomingSessionKey: 'round-001/node-b/attempt-001',
      followMode: 'manual',
    })).toBe('round-001/node-b/attempt-001');
  });

  it('selects the incoming session while auto-follow is pending after the current session naturally finished', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: 'round-001/node-a/attempt-001',
      incomingSessionKey: 'round-001/node-b/attempt-001',
      followMode: 'auto',
      currentSelectedActive: false,
      incomingActive: true,
      currentSelectedRuntimeControlled: true,
      incomingRuntimeControlled: true,
    })).toBe('round-001/node-b/attempt-001');
  });

  it('does not auto-follow after the selected session enters a manual follow-up turn', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: 'round-001/dev/attempt-001',
      incomingSessionKey: 'round-001/clean/attempt-001',
      followMode: 'auto',
      currentSelectedActive: false,
      incomingActive: true,
      currentSelectedRuntimeControlled: false,
      incomingRuntimeControlled: true,
    })).toBe('round-001/dev/attempt-001');
  });

  it('does not auto-follow into a non-runtime-controlled session', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: 'round-001/dev/attempt-001',
      incomingSessionKey: 'round-001/clean/attempt-001',
      followMode: 'auto',
      currentSelectedActive: false,
      incomingActive: true,
      currentSelectedRuntimeControlled: true,
      incomingRuntimeControlled: false,
    })).toBe('round-001/dev/attempt-001');
  });

  it('keeps the current active session while auto-follow is enabled', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: 'round-001/node-a/attempt-001',
      incomingSessionKey: 'round-001/node-b/attempt-001',
      followMode: 'auto',
      currentSelectedActive: true,
      incomingActive: true,
      currentSelectedRuntimeControlled: true,
      incomingRuntimeControlled: true,
    })).toBe('round-001/node-a/attempt-001');
  });

  it('follows an active AI-DYNAMIC sibling after the selected leaf becomes terminal', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: 'round-001/ai-dynamic/attempt-001/bootstrap/attempt-001',
      incomingSessionKey: 'round-001/ai-dynamic/attempt-001/branch-a/attempt-001',
      followMode: 'auto',
      currentSelectedActive: false,
      currentSelectedTerminal: true,
      incomingActive: true,
      currentSelectedRuntimeControlled: false,
      incomingRuntimeControlled: true,
    })).toBe('round-001/ai-dynamic/attempt-001/branch-a/attempt-001');
  });

  it('keeps the current active AI-DYNAMIC leaf when a parallel sibling starts', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: 'round-001/ai-dynamic/attempt-001/branch-a/attempt-001',
      incomingSessionKey: 'round-001/ai-dynamic/attempt-001/branch-b/attempt-001',
      followMode: 'auto',
      currentSelectedActive: true,
      currentSelectedTerminal: false,
      incomingActive: true,
      currentSelectedRuntimeControlled: true,
      incomingRuntimeControlled: true,
    })).toBe('round-001/ai-dynamic/attempt-001/branch-a/attempt-001');
  });

  it('does not follow an AI-DYNAMIC leaf from another outer attempt', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: 'round-001/ai-dynamic/attempt-001/bootstrap/attempt-001',
      incomingSessionKey: 'round-001/ai-dynamic/attempt-002/branch-a/attempt-001',
      followMode: 'auto',
      currentSelectedActive: false,
      currentSelectedTerminal: true,
      incomingActive: true,
      currentSelectedRuntimeControlled: false,
      incomingRuntimeControlled: true,
    })).toBe('round-001/ai-dynamic/attempt-001/bootstrap/attempt-001');
  });

  it('recognizes dynamic siblings only from complete inner locators', () => {
    expect(areDynamicConversationSiblingSessionKeys(
      'round-001/ai-dynamic/attempt-001/bootstrap/attempt-001',
      'round-001/ai-dynamic/attempt-001/branch-a/attempt-001',
    )).toBe(true);
    expect(areDynamicConversationSiblingSessionKeys(
      'round-001/node-a/attempt-001',
      'round-001/node-b/attempt-001',
    )).toBe(false);
  });

  it('does not let an outer AI-DYNAMIC event steal an internal selection', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: 'round-001/ai-dynamic/attempt-001/bootstrap/attempt-001',
      incomingSessionKey: 'round-001/ai-dynamic/attempt-001',
      followMode: 'auto',
    })).toBe('round-001/ai-dynamic/attempt-001/bootstrap/attempt-001');
  });

  it('preserves the current selection while manual mode is active', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: 'round-001/node-a/attempt-001',
      incomingSessionKey: 'round-001/node-b/attempt-001',
      followMode: 'manual',
    })).toBe('round-001/node-a/attempt-001');
  });

  it('does not steal focus from a manually selected historical session', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: 'round-001/history/attempt-001',
      incomingSessionKey: 'round-001/node-b/attempt-001',
      followMode: 'manual',
      currentSelectedActive: false,
      incomingActive: true,
    })).toBe('round-001/history/attempt-001');
  });

  it('enables auto-follow only for a running session at the bottom', () => {
    expect(shouldEnableConversationAutoFollow(true, true, true)).toBe(true);
    expect(shouldEnableConversationAutoFollow(true, false, true)).toBe(false);
    expect(shouldEnableConversationAutoFollow(false, true, true)).toBe(false);
    expect(shouldEnableConversationAutoFollow(true, true, false)).toBe(false);
  });

  it('keeps the manual selection when a queued live refresh runs after auto-follow is disabled', () => {
    expect(resolveConversationRefreshSelectedSessionKey({
      followMode: 'manual',
      pendingEventSessionKey: 'round-001/node-b/attempt-001',
      currentSelectedKey: 'round-001/node-a/attempt-001',
    })).toBe('round-001/node-a/attempt-001');
  });

  it('switches to the pending running session only in auto mode', () => {
    expect(resolveConversationRefreshSelectedSessionKey({
      followMode: 'auto',
      pendingEventSessionKey: 'round-001/node-b/attempt-001',
      currentSelectedKey: 'round-001/node-a/attempt-001',
      currentSelectedRuntimeControlled: true,
      pendingEventRuntimeControlled: true,
    })).toBe('round-001/node-b/attempt-001');
  });

  it('keeps the selected session during a queued refresh after a manual follow-up completes', () => {
    expect(resolveConversationRefreshSelectedSessionKey({
      followMode: 'auto',
      pendingEventSessionKey: 'round-001/clean/attempt-001',
      currentSelectedKey: 'round-001/dev/attempt-001',
      currentSelectedRuntimeControlled: false,
      pendingEventRuntimeControlled: true,
    })).toBe('round-001/dev/attempt-001');
  });

  it('does not switch a queued refresh into a non-runtime-controlled session', () => {
    expect(resolveConversationRefreshSelectedSessionKey({
      followMode: 'auto',
      pendingEventSessionKey: 'round-001/clean/attempt-001',
      currentSelectedKey: 'round-001/dev/attempt-001',
      currentSelectedRuntimeControlled: true,
      pendingEventRuntimeControlled: false,
    })).toBe('round-001/dev/attempt-001');
  });

  it('revalidates runtime control before committing an in-flight auto-follow refresh', () => {
    const requestedKey = resolveConversationRefreshSelectedSessionKey({
      followMode: 'auto',
      pendingEventSessionKey: 'round-001/clean/attempt-001',
      currentSelectedKey: 'round-001/dev/attempt-001',
      currentSelectedRuntimeControlled: true,
      pendingEventRuntimeControlled: true,
    });
    expect(requestedKey).toBe('round-001/clean/attempt-001');

    expect(resolveConversationRefreshSelectedSessionKey({
      followMode: 'auto',
      pendingEventSessionKey: requestedKey,
      currentSelectedKey: 'round-001/dev/attempt-001',
      currentSelectedRuntimeControlled: false,
      pendingEventRuntimeControlled: true,
    })).toBe('round-001/dev/attempt-001');
  });

  it('keeps a terminal AI-DYNAMIC sibling target during in-flight refresh revalidation', () => {
    const currentSelectedKey = 'round-001/ai-dynamic/attempt-001/bootstrap/attempt-001';
    const requestedKey = resolveConversationRefreshSelectedSessionKey({
      followMode: 'auto',
      pendingEventSessionKey: 'round-001/ai-dynamic/attempt-001/branch-a/attempt-001',
      currentSelectedKey,
      currentSelectedTerminal: true,
      currentSelectedRuntimeControlled: false,
      pendingEventRuntimeControlled: true,
    });
    expect(requestedKey).toBe('round-001/ai-dynamic/attempt-001/branch-a/attempt-001');

    expect(resolveConversationRefreshSelectedSessionKey({
      followMode: 'auto',
      pendingEventSessionKey: requestedKey,
      currentSelectedKey,
      currentSelectedTerminal: true,
      currentSelectedRuntimeControlled: false,
      pendingEventRuntimeControlled: true,
    })).toBe(requestedKey);
  });

  it('refreshes an outer AI-DYNAMIC event with the selected internal session key', () => {
    expect(resolveConversationRefreshSelectedSessionKey({
      followMode: 'auto',
      pendingEventSessionKey: 'round-001/ai-dynamic/attempt-001',
      currentSelectedKey: 'round-001/ai-dynamic/attempt-001/bootstrap/attempt-001',
      currentSelectedRuntimeControlled: true,
      pendingEventRuntimeControlled: true,
    })).toBe('round-001/ai-dynamic/attempt-001/bootstrap/attempt-001');
  });

  it('does not queue a run refresh for non-terminal updates from the selected session', () => {
    expect(shouldQueueConversationRunRefreshForAcpUpdate({
      treeHasSession: true,
      alreadySelected: true,
      sessionStatus: null,
    })).toBe(false);
    expect(shouldQueueConversationRunRefreshForAcpUpdate({
      treeHasSession: true,
      alreadySelected: true,
      sessionStatus: 'running',
    })).toBe(false);
  });

  it('queues a run refresh for terminal snapshots from the selected session', () => {
    for (const sessionStatus of ['completed', 'complete', 'cancelled', 'canceled', 'failed', 'failure', 'error', 'killed']) {
      expect(shouldQueueConversationRunRefreshForAcpUpdate({
        treeHasSession: true,
        alreadySelected: true,
        hasSessionSnapshot: true,
        sessionStatus,
      })).toBe(true);
    }
    expect(shouldQueueConversationRunRefreshForAcpUpdate({
      treeHasSession: true,
      alreadySelected: true,
      hasSessionSnapshot: true,
      sessionStatus: 'cancel_requested',
    })).toBe(false);
  });

  it('uses canonical runtime status for AI-DYNAMIC lifecycle refreshes', () => {
    const lifecycle = {
      displayStatus: 'idle',
      runtime: { status: 'completed' },
    } as Parameters<typeof conversationAcpRunRefreshStatus>[0]['lifecycle'];
    const refreshStatus = conversationAcpRunRefreshStatus({
      dynamicSession: true,
      lifecycle,
      sessionStatus: 'idle',
    });
    expect(refreshStatus).toBe('completed');
    expect(planConversationAcpRunUpdate({
      treeHasSession: true,
      alreadySelected: true,
      hasRuntimeSnapshot: true,
      hasLiveEvent: false,
      sessionStatus: refreshStatus,
    }).queueRunRefresh).toBe(true);
    expect(conversationAcpRunRefreshStatus({
      dynamicSession: false,
      lifecycle,
      sessionStatus: 'idle',
    })).toBe('idle');
  });

  it('ignores high-frequency live events from a known background session', () => {
    expect(planConversationAcpRunUpdate({
      treeHasSession: true,
      alreadySelected: false,
      hasSessionSnapshot: false,
      hasLiveEvent: true,
      sessionStatus: null,
    })).toEqual({
      patchSelectedSession: false,
      patchBackgroundSession: false,
      queueRunRefresh: false,
    });
  });

  it('queues a refresh for known background live events only while auto-follow is pending', () => {
    expect(planConversationAcpRunUpdate({
      treeHasSession: true,
      alreadySelected: false,
      hasSessionSnapshot: false,
      hasLiveEvent: true,
      sessionStatus: null,
      followPending: true,
    })).toEqual({
      patchSelectedSession: false,
      patchBackgroundSession: false,
      queueRunRefresh: true,
    });
  });

  it('lightly patches non-terminal background session snapshots without queueing a full refresh', () => {
    expect(planConversationAcpRunUpdate({
      treeHasSession: true,
      alreadySelected: false,
      hasSessionSnapshot: true,
      hasLiveEvent: false,
      sessionStatus: 'running',
    })).toEqual({
      patchSelectedSession: false,
      patchBackgroundSession: true,
      queueRunRefresh: false,
    });
  });

  it('queues a refresh for lifecycle-only background snapshots while auto-follow is pending', () => {
    expect(planConversationAcpRunUpdate({
      treeHasSession: true,
      alreadySelected: false,
      hasRuntimeSnapshot: true,
      hasLiveEvent: false,
      sessionStatus: 'ready',
      followPending: true,
    })).toEqual({
      patchSelectedSession: false,
      patchBackgroundSession: true,
      queueRunRefresh: true,
    });
  });

  it('queues a full refresh when a background session reaches an interactive or terminal state', () => {
    expect(planConversationAcpRunUpdate({
      treeHasSession: true,
      alreadySelected: false,
      hasSessionSnapshot: true,
      hasLiveEvent: false,
      sessionStatus: 'running',
      pendingPermissionCount: 1,
    })).toMatchObject({ patchBackgroundSession: false, queueRunRefresh: true });
    expect(planConversationAcpRunUpdate({
      treeHasSession: true,
      alreadySelected: false,
      hasSessionSnapshot: true,
      hasLiveEvent: false,
      sessionStatus: 'completed',
    })).toMatchObject({ patchBackgroundSession: false, queueRunRefresh: true });
  });

  it('queues a full refresh when a live event belongs to a session missing from the tree', () => {
    expect(planConversationAcpRunUpdate({
      treeHasSession: false,
      alreadySelected: false,
      hasSessionSnapshot: false,
      hasLiveEvent: true,
    })).toMatchObject({ queueRunRefresh: true });
  });

  it('resets run-page auto-follow only when the run id changes', () => {
    expect(runPageResetCount(['run-1', 'run-1', 'run-1'])).toBe(1);
    expect(runPageResetCount(['run-1', 'run-1', 'run-2'])).toBe(2);
  });
});
