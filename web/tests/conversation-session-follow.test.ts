import { describe, expect, it } from 'vitest';
import {
  resolveConversationEventSelectedSessionKey,
  shouldEnableConversationAutoFollow,
} from '@/lib/conversation-session-follow';

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
    expect(shouldEnableConversationAutoFollow('running', true)).toBe(true);
    expect(shouldEnableConversationAutoFollow('running', false)).toBe(false);
    expect(shouldEnableConversationAutoFollow('success', true)).toBe(false);
  });
});
