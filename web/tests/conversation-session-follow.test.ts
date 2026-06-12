import { describe, expect, it } from 'vitest';
import {
  resolveConversationEventSelectedSessionKey,
  resolveConversationRefreshSelectedSessionKey,
  shouldEnableConversationAutoFollow,
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
      autoFollow: false,
    })).toBe('round-001/node-b/attempt-001');
  });

  it('selects the incoming session while auto-follow is enabled', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: 'round-001/node-a/attempt-001',
      incomingSessionKey: 'round-001/node-b/attempt-001',
      autoFollow: true,
    })).toBe('round-001/node-b/attempt-001');
  });

  it('preserves the current selection while auto-follow is disabled', () => {
    expect(resolveConversationEventSelectedSessionKey({
      currentSelectedKey: 'round-001/node-a/attempt-001',
      incomingSessionKey: 'round-001/node-b/attempt-001',
      autoFollow: false,
    })).toBe('round-001/node-a/attempt-001');
  });

  it('enables auto-follow only for a running session at the bottom', () => {
    expect(shouldEnableConversationAutoFollow(true, true)).toBe(true);
    expect(shouldEnableConversationAutoFollow(true, false)).toBe(false);
    expect(shouldEnableConversationAutoFollow(false, true)).toBe(false);
  });

  it('keeps the manual selection when a queued live refresh runs after auto-follow is disabled', () => {
    expect(resolveConversationRefreshSelectedSessionKey({
      autoFollow: false,
      pendingEventSessionKey: 'round-001/node-b/attempt-001',
      currentSelectedKey: 'round-001/node-a/attempt-001',
    })).toBe('round-001/node-a/attempt-001');
  });

  it('still switches to the pending running session while auto-follow remains enabled', () => {
    expect(resolveConversationRefreshSelectedSessionKey({
      autoFollow: true,
      pendingEventSessionKey: 'round-001/node-b/attempt-001',
      currentSelectedKey: 'round-001/node-a/attempt-001',
    })).toBe('round-001/node-b/attempt-001');
  });

  it('resets run-page auto-follow only when the run id changes', () => {
    expect(runPageResetCount(['run-1', 'run-1', 'run-1'])).toBe(1);
    expect(runPageResetCount(['run-1', 'run-1', 'run-2'])).toBe(2);
  });
});
