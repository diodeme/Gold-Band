import { describe, expect, it } from 'vitest';
import {
  resolveConversationContentQueryState,
  shouldShowConversationContentLoadingState,
} from '../src/pages/ConversationRunPage';

describe('ConversationRunPage content loading gate', () => {
  it('shows hydrated cached content immediately while it revalidates', () => {
    expect(resolveConversationContentQueryState('session-a', null, true)).toBe('success');
  });

  it('loads an uncached identity and ignores a previous session projection', () => {
    expect(resolveConversationContentQueryState(
      'session-b',
      { identity: 'session-a', state: 'success' },
      false,
    )).toBe('loading');
  });

  it('keeps a current session on its runtime-projected chat shell', () => {
    expect(shouldShowConversationContentLoadingState('loading', { current: true })).toBe(false);
  });

  it('keeps the isolated content loading state for an uncached historical session', () => {
    expect(shouldShowConversationContentLoadingState('loading', { current: false })).toBe(true);
  });

  it('does not show the content loading state after hydration completes', () => {
    expect(shouldShowConversationContentLoadingState('success', { current: true })).toBe(false);
  });
});
