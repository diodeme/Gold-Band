import { describe, expect, it } from 'vitest';
import {
  isAcpSessionInitializationFailed,
  isAcpSessionInitializationInterrupted,
  missingAcpSessionRetryDelay,
  resolveAcpSessionShellState,
  shouldCreateLiveAcpSessionShell,
} from '@/lib/acp-session-shell';

describe('shouldCreateLiveAcpSessionShell', () => {
  it('does not create a shell for runtime-active conversation owners that disable event-only fallback', () => {
    expect(shouldCreateLiveAcpSessionShell({
      runtimeActive: true,
      allowEventOnlySessionShell: false,
      loadedEventCount: 0,
    })).toBe(false);
  });

  it('does not create a running shell from existing events when the owner disables event-only fallback', () => {
    expect(shouldCreateLiveAcpSessionShell({
      runtimeActive: false,
      allowEventOnlySessionShell: false,
      loadedEventCount: 3,
    })).toBe(false);
  });

  it('creates a runtime-active shell when the owner explicitly allows event-only fallback', () => {
    expect(shouldCreateLiveAcpSessionShell({
      runtimeActive: true,
      allowEventOnlySessionShell: true,
      loadedEventCount: 0,
    })).toBe(true);
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
  it('shows runtime initialization errors before the loading shell', () => {
    expect(resolveAcpSessionShellState({
      hasBaseSession: true,
      baseSessionReady: false,
      hasLiveSessionShell: false,
      initialSessionLoading: true,
      initializationFailed: true,
    })).toBe('error');
  });

  it('shows an interrupted terminal shell immediately when ACP initialization never established a session', () => {
    expect(resolveAcpSessionShellState({
      hasBaseSession: true,
      baseSessionReady: false,
      hasLiveSessionShell: false,
      initialSessionLoading: true,
      initializationInterrupted: true,
    })).toBe('interrupted');
  });

  it('keeps session switching in loading state until the target session fetch resolves', () => {
    expect(resolveAcpSessionShellState({
      hasBaseSession: false,
      baseSessionReady: false,
      hasLiveSessionShell: false,
      initialSessionLoading: true,
    })).toBe('loading');
  });

  it('keeps partial base sessions in loading state while the initial ready fetch is in flight', () => {
    expect(resolveAcpSessionShellState({
      hasBaseSession: true,
      baseSessionReady: false,
      hasLiveSessionShell: false,
      initialSessionLoading: true,
    })).toBe('loading');
  });

  it('treats ready session payloads and live shells as available', () => {
    expect(resolveAcpSessionShellState({
      hasBaseSession: true,
      baseSessionReady: true,
      hasLiveSessionShell: false,
      initialSessionLoading: true,
    })).toBe('available');
    expect(resolveAcpSessionShellState({
      hasBaseSession: false,
      baseSessionReady: false,
      hasLiveSessionShell: true,
      initialSessionLoading: true,
    })).toBe('available');
  });

  it('reports missing only after loading has completed without a session', () => {
    expect(resolveAcpSessionShellState({
      hasBaseSession: false,
      baseSessionReady: false,
      hasLiveSessionShell: false,
      initialSessionLoading: false,
    })).toBe('missing');
  });

  it('shows runtime-active empty session owners in an initializing shell', () => {
    expect(resolveAcpSessionShellState({
      hasBaseSession: false,
      baseSessionReady: false,
      hasLiveSessionShell: false,
      initialSessionLoading: true,
      runtimeActive: true,
      showInitializingShell: true,
    })).toBe('initializing');
  });

  it('keeps a durably established session available while its detail payload is temporarily absent', () => {
    expect(resolveAcpSessionShellState({
      hasBaseSession: false,
      baseSessionReady: false,
      hasLiveSessionShell: false,
      hasEstablishedSessionShell: true,
      initialSessionLoading: false,
    })).toBe('available');
  });

  it('keeps runtime-active session switching in loading without initialization ownership', () => {
    expect(resolveAcpSessionShellState({
      hasBaseSession: false,
      baseSessionReady: false,
      hasLiveSessionShell: false,
      initialSessionLoading: true,
      runtimeActive: true,
      showInitializingShell: false,
    })).toBe('loading');
  });

  it('keeps a current partial session in the initializing shell until metadata is ready', () => {
    expect(resolveAcpSessionShellState({
      hasBaseSession: true,
      baseSessionReady: false,
      hasLiveSessionShell: false,
      initialSessionLoading: true,
      runtimeActive: true,
      showInitializingShell: true,
    })).toBe('initializing');
  });

  it('allows partial base sessions after startup retries are exhausted', () => {
    expect(resolveAcpSessionShellState({
      hasBaseSession: true,
      baseSessionReady: false,
      hasLiveSessionShell: false,
      initialSessionLoading: false,
    })).toBe('available');
  });
});

describe('isAcpSessionInitializationFailed', () => {
  const failedInput = {
    runtimeStatus: 'paused',
    runtimePauseReason: 'error-blocked',
    runtimeActive: false,
    runtimeComposerMode: 'runtime-error',
    runtimeErrorMessage: 'Configured model is unavailable',
    sessionId: null,
    baseSessionReady: false,
    loadedEventCount: 0,
  };

  it('identifies runtime errors that happen before ACP session-ready state', () => {
    expect(isAcpSessionInitializationFailed(failedInput)).toBe(true);
  });

  it('ends loading when a resumable runtime-abnormal pause happens before session creation', () => {
    expect(isAcpSessionInitializationFailed({
      ...failedInput,
      runtimePauseReason: 'runtime-abnormal',
      runtimeComposerMode: 'normal',
      runtimeErrorMessage: "Codex doesn't support MCP SSE transport protocol",
    })).toBe(true);
  });

  it('uses canonical failed runtime status even when no ACP composer error was created', () => {
    expect(isAcpSessionInitializationFailed({
      ...failedInput,
      runtimeStatus: 'failed',
      runtimePauseReason: null,
      runtimeComposerMode: 'normal',
      runtimeErrorMessage: null,
    })).toBe(true);
  });

  it('keeps established or active sessions on the normal conversation path', () => {
    expect(isAcpSessionInitializationFailed({
      ...failedInput,
      sessionId: 'session-1',
    })).toBe(false);
    expect(isAcpSessionInitializationFailed({
      ...failedInput,
      runtimeActive: true,
    })).toBe(false);
    expect(isAcpSessionInitializationFailed({
      ...failedInput,
      loadedEventCount: 1,
    })).toBe(false);
  });

  it('does not turn non-error pauses into ACP initialization failures', () => {
    expect(isAcpSessionInitializationFailed({
      ...failedInput,
      runtimePauseReason: 'waiting-for-user-input',
      runtimeComposerMode: 'normal',
    })).toBe(false);
  });
});

describe('isAcpSessionInitializationInterrupted', () => {
  const interruptedInput = {
    runtimeStatus: 'paused',
    runtimePauseReason: 'process-interrupted',
    runtimeActive: false,
    sessionId: null,
    baseSessionReady: false,
    loadedEventCount: 0,
  };

  it('identifies a stopped runtime attempt that never established an ACP session', () => {
    expect(isAcpSessionInitializationInterrupted(interruptedInput)).toBe(true);
  });

  it('keeps established or displayable interrupted sessions on the normal conversation path', () => {
    expect(isAcpSessionInitializationInterrupted({
      ...interruptedInput,
      sessionEstablished: true,
    })).toBe(false);
    expect(isAcpSessionInitializationInterrupted({
      ...interruptedInput,
      sessionId: 'session-1',
    })).toBe(false);
    expect(isAcpSessionInitializationInterrupted({
      ...interruptedInput,
      baseSessionReady: true,
    })).toBe(false);
    expect(isAcpSessionInitializationInterrupted({
      ...interruptedInput,
      loadedEventCount: 1,
    })).toBe(false);
  });

  it('still identifies an outbound-only session/new attempt as interrupted', () => {
    expect(isAcpSessionInitializationInterrupted({
      ...interruptedInput,
      sessionEstablished: false,
    })).toBe(true);
  });

  it('does not replace an active startup or another pause reason with interrupted', () => {
    expect(isAcpSessionInitializationInterrupted({
      ...interruptedInput,
      runtimeActive: true,
    })).toBe(false);
    expect(isAcpSessionInitializationInterrupted({
      ...interruptedInput,
      runtimePauseReason: 'waiting-for-user-input',
    })).toBe(false);
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
