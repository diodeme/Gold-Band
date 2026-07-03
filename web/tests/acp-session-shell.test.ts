import { describe, expect, it } from 'vitest';
import {
  missingAcpSessionRetryDelay,
  resolveAcpSessionShellState,
  shouldCreateLiveAcpSessionShell,
} from '@/lib/acp-session-shell';

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

describe('missingAcpSessionRetryDelay', () => {
  it('returns a positive delay for the first retry attempt', () => {
    expect(missingAcpSessionRetryDelay(0)).toBeGreaterThan(0);
  });

  it('returns null when retry attempts are exhausted', () => {
    expect(missingAcpSessionRetryDelay(11)).toBeNull();
  });

  it('returns increasing delays before settling into the long-poll interval', () => {
    const d0 = missingAcpSessionRetryDelay(0);
    const d1 = missingAcpSessionRetryDelay(1);
    const d2 = missingAcpSessionRetryDelay(2);
    const d3 = missingAcpSessionRetryDelay(3);
    const d4 = missingAcpSessionRetryDelay(4);
    const d5 = missingAcpSessionRetryDelay(5);
    const d6 = missingAcpSessionRetryDelay(6);
    expect(d0).toBeGreaterThan(0);
    expect(d1).toBeGreaterThan(d0!);
    expect(d2).toBeGreaterThan(d1!);
    expect(d3).toBeGreaterThan(d2!);
    expect(d4).toBeGreaterThan(d3!);
    expect(d5).toBeGreaterThan(d4!);
    expect(d6).toBeGreaterThan(d5!);
    expect(missingAcpSessionRetryDelay(7)).toBe(d6);
  });

  it('keeps polling long enough for slow ACP session startup', () => {
    let totalDelayMs = 0;
    for (let attempt = 0; ; attempt += 1) {
      const delay = missingAcpSessionRetryDelay(attempt);
      if (delay == null) break;
      totalDelayMs += delay;
    }

    expect(totalDelayMs).toBeGreaterThanOrEqual(30_000);
  });
});
