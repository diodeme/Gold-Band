import { afterEach, describe, expect, it, vi } from 'vitest';
import { sha256Hex } from '@/lib/sha256';

const ABC = new TextEncoder().encode('abc').buffer as ArrayBuffer;
const ABC_SHA256 = 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad';

describe('sha256Hex', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('matches the published vector', async () => {
    expect(await sha256Hex(ABC)).toBe(ABC_SHA256);
  });

  // A plain-HTTP deployment has no crypto.subtle, and losing the asset integrity check there
  // would leave the recorded replays and the demo dataset unverifiable.
  it('keeps hashing on an origin without crypto.subtle', async () => {
    vi.stubGlobal('crypto', { subtle: undefined });
    expect(await sha256Hex(ABC)).toBe(ABC_SHA256);
  });
});
