/** @vitest-environment jsdom */
import { describe, expect, it, beforeEach, afterEach } from 'vitest';
import { detachedTerminalSessionId, isDetachedLabel, parseDetachedRoute } from '../src/lib/detached';

describe('detached window route parsing', () => {
  const originalLocation = window.location;

  beforeEach(() => {
    // parseDetachedRoute reads window.location.search; jsdom lets us stub it.
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: { ...originalLocation, search: '' },
    });
  });

  afterEach(() => {
    Object.defineProperty(window, 'location', { configurable: true, value: originalLocation });
  });

  it('is not detached without a window query param', () => {
    (window.location as unknown as { search: string }).search = '';
    expect(parseDetachedRoute()).toEqual({ isDetached: false, kind: null, sessionId: null });
  });

  it('parses a detached terminal route with its session id', () => {
    (window.location as unknown as { search: string }).search = '?window=terminal&session=sess-abc';
    expect(parseDetachedRoute()).toEqual({ isDetached: true, kind: 'terminal', sessionId: 'sess-abc' });
  });

  it('parses a detached connections route', () => {
    (window.location as unknown as { search: string }).search = '?window=connections';
    expect(parseDetachedRoute()).toEqual({ isDetached: true, kind: 'connections', sessionId: null });
  });
});

describe('detached window label parsing', () => {
  it('extracts a session id from a detached terminal label', () => {
    expect(detachedTerminalSessionId('detached-terminal-sess-abc')).toBe('sess-abc');
    expect(detachedTerminalSessionId('detached-connections')).toBeNull();
  });

  it('recognizes detached labels', () => {
    expect(isDetachedLabel('detached-terminal-x')).toBe(true);
    expect(isDetachedLabel('main')).toBe(false);
  });
});
