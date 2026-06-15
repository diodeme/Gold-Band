import { describe, expect, it } from 'vitest';
import { extractErrorMessage, extractErrorStack, formatUiErrorDiagnostic, shouldLogUiError } from '@/lib/ui-error-diagnostics';

describe('ui error diagnostics', () => {
  it('matches maximum update depth errors from message or stack', () => {
    expect(shouldLogUiError(new Error('Maximum update depth exceeded. This can happen when a component repeatedly calls setState.'))).toBe(true);
    expect(shouldLogUiError({
      message: 'Uncaught error',
      stack: 'Error: boom\nMaximum update depth exceeded\n    at setRef',
    })).toBe(true);
  });

  it('ignores unrelated errors', () => {
    expect(shouldLogUiError(new Error('Network request failed'))).toBe(false);
    expect(shouldLogUiError('Permission denied')).toBe(false);
  });

  it('extracts message and stack from error-like values', () => {
    const errorLike = {
      message: 'Maximum update depth exceeded',
      stack: 'at composeRefs',
    };

    expect(extractErrorMessage(errorLike)).toBe('Maximum update depth exceeded');
    expect(extractErrorStack(errorLike)).toBe('at composeRefs');
  });

  it('formats diagnostics as copyable text instead of collapsed console objects', () => {
    expect(formatUiErrorDiagnostic({
      message: 'Maximum update depth exceeded',
      componentStack: 'at TooltipTrigger',
    })).toContain('componentStack=at TooltipTrigger');
  });
});
