import { describe, expect, it } from 'vitest';
import { resolveAcpSessionShellState, shouldCreateLiveAcpSessionShell } from '@/lib/acp-session-shell';

describe('shouldCreateLiveAcpSessionShell', () => {
  it('creates a shell when runtime is active even before session payload exists', () => {
    expect(shouldCreateLiveAcpSessionShell({
      runtimeActive: true,
      allowEventOnlySessionShell: false,
      loadedEventCount: 0,
    })).toBe(true);
  });

  it('does not create a running shell from existing events when the owner disables event-only fallback', () => {
    expect(shouldCreateLiveAcpSessionShell({
      runtimeActive: false,
      allowEventOnlySessionShell: false,
      loadedEventCount: 3,
    })).toBe(false);
  });

  it('keeps the legacy event-only fallback available for non-conversation owners', () => {
    expect(shouldCreateLiveAcpSessionShell({
      runtimeActive: false,
      allowEventOnlySessionShell: true,
      loadedEventCount: 3,
    })).toBe(true);
  });
});

describe('resolveAcpSessionShellState', () => {
  it('keeps session switching in loading state until the target session fetch resolves', () => {
    expect(resolveAcpSessionShellState({
      hasBaseSession: false,
      hasLiveSessionShell: false,
      initialSessionLoading: true,
    })).toBe('loading');
  });

  it('treats real session payloads and live shells as available', () => {
    expect(resolveAcpSessionShellState({
      hasBaseSession: true,
      hasLiveSessionShell: false,
      initialSessionLoading: true,
    })).toBe('available');
    expect(resolveAcpSessionShellState({
      hasBaseSession: false,
      hasLiveSessionShell: true,
      initialSessionLoading: true,
    })).toBe('available');
  });

  it('reports missing only after loading has completed without a session', () => {
    expect(resolveAcpSessionShellState({
      hasBaseSession: false,
      hasLiveSessionShell: false,
      initialSessionLoading: false,
    })).toBe('missing');
  });
});
