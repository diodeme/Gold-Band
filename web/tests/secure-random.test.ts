import { describe, expect, it, vi } from 'vitest';
import { randomId } from '../src/lib/secure-random';

const V4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

describe('identifiers on secure and plain-HTTP origins', () => {
  it('uses the native v4 identifier when the platform exposes one', () => {
    expect(randomId()).toMatch(V4);
  });
  it('keeps generating unique v4 identifiers where randomUUID is unavailable', () => {
    const source = globalThis.crypto;
    vi.stubGlobal('crypto', { getRandomValues: source.getRandomValues.bind(source) });
    try {
      expect(globalThis.crypto.randomUUID).toBeUndefined();
      const ids = Array.from({ length: 64 }, () => randomId());
      expect(ids.every(id => V4.test(id))).toBe(true);
      expect(new Set(ids).size).toBe(ids.length);
    } finally { vi.unstubAllGlobals(); }
  });
});
