// @vitest-environment jsdom

import { describe, expect, it, vi } from 'vitest';
import {
  buildFrontendErrorReport,
  createUiErrorReporter,
  extractErrorMessage,
  extractErrorStack,
  formatUiErrorDiagnostic,
  installUiErrorDiagnostics,
  shouldLogUiError,
} from '@/lib/ui-error-diagnostics';

describe('ui error diagnostics', () => {
  it('routes window, unhandled rejection, and React uncaught errors through one structured sink', () => {
    const sink = vi.fn(() => Promise.resolve());
    const diagnostics = installUiErrorDiagnostics(sink);
    expect(diagnostics).not.toBeNull();

    window.dispatchEvent(new ErrorEvent('error', {
      error: new Error('window render failed'),
      filename: 'main.js',
      lineno: 18,
      colno: 7,
    }));
    const rejection = new Event('unhandledrejection');
    Object.defineProperty(rejection, 'reason', { value: new Error('async render failed') });
    window.dispatchEvent(rejection);
    diagnostics?.report('react-uncaught', new Error('React tree failed'), {
      componentStack: 'at ConversationPage',
    });

    expect(sink).toHaveBeenCalledTimes(3);
    expect(sink.mock.calls.map(([input]) => input.kind)).toEqual([
      'window-error',
      'unhandled-rejection',
      'react-uncaught',
    ]);
    expect(sink.mock.calls[0][0]).toMatchObject({
      message: 'window render failed',
      source: 'main.js',
      line: 18,
      column: 7,
    });
    expect(sink.mock.calls[2][0]).toMatchObject({ componentStack: 'at ConversationPage' });

    diagnostics?.dispose();
  });

  it('bounds every large diagnostic field before crossing the frontend interface', () => {
    const report = buildFrontendErrorReport(
      'react-uncaught',
      'm'.repeat(5_000),
      's'.repeat(20_000),
      { componentStack: 'c'.repeat(20_000), source: `${'f'.repeat(3_000)}?token=secret` },
      {
        activeElement: 'a'.repeat(2_000),
        lastPointerTarget: 'p'.repeat(2_000),
        lastPointerAt: 't'.repeat(100),
        pathname: '/'.repeat(3_000),
        userAgent: 'u'.repeat(3_000),
      },
    );

    expect(report.message).toHaveLength(4_096);
    expect(report.stack).toHaveLength(16_384);
    expect(report.componentStack).toHaveLength(16_384);
    expect(report.source).toHaveLength(2_048);
    expect(report.source).not.toContain('token=secret');
    expect(report.activeElement).toHaveLength(1_024);
    expect(report.lastPointerTarget).toHaveLength(1_024);
    expect(report.lastPointerAt).toHaveLength(64);
    expect(report.pathname).toHaveLength(2_048);
    expect(report.userAgent).toHaveLength(2_048);
  });

  it('deduplicates repeated failures and throttles distinct error storms', () => {
    const sink = vi.fn();
    let now = 0;
    const report = createUiErrorReporter(sink, undefined, () => now);

    const duplicate = new Error('same failure');
    report('window-error', duplicate);
    report('react-uncaught', duplicate);
    expect(sink).toHaveBeenCalledTimes(1);

    for (let index = 0; index < 10; index += 1) {
      report('window-error', new Error(`distinct failure ${index}`));
    }
    expect(sink).toHaveBeenCalledTimes(5);

    now = 10_000;
    report('window-error', new Error('after throttle window'));
    expect(sink).toHaveBeenCalledTimes(6);
  });

  it('keeps reporting best-effort when the sink throws or rejects', async () => {
    const throwing = createUiErrorReporter(() => { throw new Error('IPC unavailable'); });
    const rejecting = createUiErrorReporter(() => Promise.reject(new Error('IPC unavailable')));
    const sinkWithBrokenContext = vi.fn();
    const brokenContext = createUiErrorReporter(sinkWithBrokenContext, () => {
      throw new Error('document unavailable');
    });

    expect(() => throwing('window-error', new Error('render failed'))).not.toThrow();
    expect(() => rejecting('window-error', new Error('render failed'))).not.toThrow();
    expect(() => brokenContext('window-error', new Error('render failed'))).not.toThrow();
    expect(sinkWithBrokenContext).toHaveBeenCalledWith(expect.objectContaining({
      activeElement: null,
      pathname: null,
    }));
    await Promise.resolve();
  });

  it('retains the maximum update depth console diagnostic', () => {
    expect(shouldLogUiError(new Error('Maximum update depth exceeded.'))).toBe(true);
    expect(shouldLogUiError({
      message: 'Uncaught error',
      stack: 'Error: boom\nMaximum update depth exceeded\n    at setRef',
    })).toBe(true);
    expect(shouldLogUiError(new Error('Network request failed'))).toBe(false);
  });

  it('extracts error-like values and formats a copyable diagnostic', () => {
    const errorLike = { message: 'render failed', stack: 'at composeRefs' };
    expect(extractErrorMessage(errorLike)).toBe('render failed');
    expect(extractErrorStack(errorLike)).toBe('at composeRefs');
    expect(formatUiErrorDiagnostic(buildFrontendErrorReport(
      'react-uncaught',
      'Maximum update depth exceeded',
      null,
      { componentStack: 'at TooltipTrigger' },
    ))).toContain('componentStack=at TooltipTrigger');
  });
});
