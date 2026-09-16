import { afterEach, expect, it, vi } from 'vitest';
import { AcpImageCache, acpActivityImages, acpImageKey } from '@/lib/acp-image-cache';
import type { TurnFileLocatorVm } from '@/types';

afterEach(() => vi.restoreAllMocks());

it('shares one request and object URL across process and result thumbnails and revokes on eviction', async () => {
  vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:shared');
  const revoke = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
  const cache = new AcpImageCache(1, 100);
  const load = vi.fn(async () => new Blob(['preview'], { type: 'image/png' }));
  const first = cache.acquire('same', load);
  const second = cache.acquire('same', load);
  expect(await first.promise).toBe(await second.promise);
  expect(load).toHaveBeenCalledTimes(1);
  first.release();
  expect(() => cache.acquire('other', load)).toThrow();
  second.release();
  const next = cache.acquire('other', load);
  await next.promise;
  expect(revoke).toHaveBeenCalledWith('blob:shared');
  next.release(); cache.clearUnused();
  expect(revoke).toHaveBeenCalledTimes(2);
});

it('bounds byte admission and releases failed requests for retry', async () => {
  const create = vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:small');
  vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
  const cache = new AcpImageCache(2, 3);
  const large = cache.acquire('image', async () => new Blob(['large']));
  await expect(large.promise).rejects.toMatchObject({ code: 'acp.image-cache-full' });
  large.release();
  expect(create).not.toHaveBeenCalled();
  const small = cache.acquire('image', async () => new Blob(['ok']));
  await expect(small.promise).resolves.toMatchObject({ url: 'blob:small' });
  small.release(); cache.clearUnused();
});

it('isolates project, branch, outer attempt, revision, and thumbnail from original', () => {
  const locator: TurnFileLocatorVm = { projectId: 'p', taskId: 't', runId: 'r', roundId: 'round', nodeId: 'n', attemptId: 'a', branchId: 'root' };
  const image = { eventId: 'event', pointer: '/content/0/content', contentHash: 'hash', mimeType: 'image/png' };
  const key = acpImageKey(locator, image, true);
  for (const modified of [{ ...locator, projectId: 'other' }, { ...locator, branchId: 'agent' }, { ...locator, outerAttemptId: 'outer' }]) {
    expect(acpImageKey(modified, image, true)).not.toBe(key);
  }
  expect(acpImageKey(locator, image, false)).not.toBe(key);
  expect(acpImageKey(locator, { ...image, contentHash: 'new' }, true)).not.toBe(key);
});

it('replaces a summary image group with the latest tool projection without duplicate images', () => {
  const image = { eventId: 'event', pointer: '/rawOutput/result/content/0', contentHash: 'hash', mimeType: 'image/png' };
  const updated = { ...image, pointer: '/content/0/content' };
  expect(acpActivityImages([
    { kind: 'activitySummary', raw: { goldBandActivity: { images: [image] } } },
    { kind: 'toolCall', raw: { goldBandImages: [updated] } },
  ])).toEqual([updated]);
});
