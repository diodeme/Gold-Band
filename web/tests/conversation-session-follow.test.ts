import { describe, expect, it } from 'vitest';
import {
  planConversationAcpRunUpdate,
  resolveConversationEventSelectedSessionKey,
  resolveConversationRefreshSelectedSessionKey,
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
  it('selects the incoming session when there is no current selection', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: null,
      incomingSessionKey: 'round-001/node-b/attempt-001',
      followMode: 'manual',
    })).toBe('round-001/node-b/attempt-001');
  });

  it('selects the incoming session while auto-follow is enabled', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: 'round-001/node-a/attempt-001',
      incomingSessionKey: 'round-001/node-b/attempt-001',
      followMode: 'auto',
    })).toBe('round-001/node-b/attempt-001');
  });

  it('preserves the current selection while manual mode is active', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: 'round-001/node-a/attempt-001',
      incomingSessionKey: 'round-001/node-b/attempt-001',
      followMode: 'manual',
    })).toBe('round-001/node-a/attempt-001');
  });

  it('enables auto-follow only for a running session at the bottom', () => {
    expect(shouldEnableConversationAutoFollow(true, true)).toBe(true);
    expect(shouldEnableConversationAutoFollow(true, false)).toBe(false);
    expect(shouldEnableConversationAutoFollow(false, true)).toBe(false);
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
    })).toBe('round-001/node-b/attempt-001');
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
