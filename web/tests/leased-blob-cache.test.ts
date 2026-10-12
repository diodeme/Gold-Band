import { afterEach, expect, it, vi } from 'vitest';
import { LeasedBlobCache } from '@/lib/leased-blob-cache';

afterEach(() => vi.restoreAllMocks());

const errors = {
  full: { code: 'test.full', params: {} },
  cancelled: { code: 'test.cancelled', params: {} },
};

it('exposes a ready asset synchronously through peek and keeps its metadata', async () => {
  vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:ready');
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
  const cache = new LeasedBlobCache<{ width: number }>({ maxEntries: 4, maxBytes: 100, concurrency: 1, errors });
  expect(cache.peek('diagram')).toBeNull();
  const lease = cache.acquire('diagram', async () => ({ blob: new Blob(['svg']), meta: { width: 120 } }));
  expect(cache.peek('diagram')).toBeNull();
  await lease.promise;
  expect(cache.peek('diagram')).toMatchObject({ url: 'blob:ready', meta: { width: 120 } });
  lease.release();
});

it('runs loads one at a time when concurrency is one', async () => {
  vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:serial');
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
  const cache = new LeasedBlobCache<void>({ maxEntries: 4, maxBytes: 100, concurrency: 1, errors });
  let running = 0;
  let maxRunning = 0;
  const load = async () => {
    running += 1;
    maxRunning = Math.max(maxRunning, running);
    await new Promise((resolve) => setTimeout(resolve, 5));
    running -= 1;
    return { blob: new Blob(['x']), meta: undefined };
  };
  const leases = ['a', 'b', 'c'].map((key) => cache.acquire(key, load));
  await Promise.all(leases.map((lease) => lease.promise));
  expect(maxRunning).toBe(1);
  leases.forEach((lease) => lease.release());
});

it('never evicts an entry while a consumer still holds its lease', async () => {
  vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:held');
  const revoke = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
  const cache = new LeasedBlobCache<void>({ maxEntries: 1, maxBytes: 100, concurrency: 1, errors });
  const held = cache.acquire('held', async () => ({ blob: new Blob(['x']), meta: undefined }));
  await held.promise;
  expect(() => cache.acquire('other', async () => ({ blob: new Blob(['y']), meta: undefined }))).toThrow();
  expect(revoke).not.toHaveBeenCalled();
  held.release();
});
