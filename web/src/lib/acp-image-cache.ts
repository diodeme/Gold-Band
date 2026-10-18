import { getAcpImage } from '@/api';
import type { AcpImageRef, TurnFileLocatorVm } from '@/types';
import { LeasedBlobCache, type LeasedBlobAsset } from '@/lib/leased-blob-cache';

export const ACP_IMAGE_CACHE_ENTRIES = 80;
export const ACP_IMAGE_CACHE_BYTES = 48 * 1024 * 1024;
export const ACP_PROJECTED_IMAGE_LIMIT = 256;
const IMAGE_REQUEST_CONCURRENCY = 2;

export function acpImageKey(locator: TurnFileLocatorVm, image: AcpImageRef, thumbnail: boolean) {
  return JSON.stringify([locator.projectId, locator.taskId, locator.runId, locator.roundId, locator.nodeId,
    locator.attemptId, locator.outerNodeId, locator.outerAttemptId, locator.branchId,
    image.eventId, image.pointer, image.contentHash, thumbnail]);
}

export type AcpImageAsset = LeasedBlobAsset<void>;

export class AcpImageCache {
  private readonly cache: LeasedBlobCache<void>;

  constructor(maxEntries = ACP_IMAGE_CACHE_ENTRIES, maxBytes = ACP_IMAGE_CACHE_BYTES) {
    this.cache = new LeasedBlobCache({
      maxEntries,
      maxBytes,
      concurrency: IMAGE_REQUEST_CONCURRENCY,
      errors: {
        full: { code: 'acp.image-cache-full', params: {} },
        cancelled: { code: 'acp.image-cancelled', params: {} },
      },
    });
  }

  acquire(key: string, load: () => Promise<Blob>) {
    return this.cache.acquire(key, async () => ({ blob: await load(), meta: undefined }));
  }

  clearUnused() {
    this.cache.clearUnused();
  }
}

const cache = new AcpImageCache();
export function acquireAcpImage(locator: TurnFileLocatorVm, image: AcpImageRef, thumbnail: boolean) {
  return cache.acquire(acpImageKey(locator, image, thumbnail), async () => {
    const content = await getAcpImage(locator, image, thumbnail);
    return (await fetch(content.dataUrl)).blob();
  });
}

export async function loadAcpOriginalImage(locator: TurnFileLocatorVm, image: AcpImageRef) {
  const lease = acquireAcpImage(locator, image, false);
  try { return (await lease.promise).blob; } finally { lease.release(); }
}

export function acpImagesFromRaw(raw: unknown): AcpImageRef[] {
  const value = raw as { goldBandImages?: AcpImageRef[]; goldBandActivity?: { images?: AcpImageRef[] } } | null;
  const images = value?.goldBandImages ?? value?.goldBandActivity?.images;
  return Array.isArray(images) ? images : [];
}

export function acpActivityImages(events: Array<{ kind: string; raw?: unknown }>): AcpImageRef[] {
  const byEvent = new Map<string, AcpImageRef[]>();
  for (const event of events) {
    const grouped = new Map<string, AcpImageRef[]>();
    for (const image of acpImagesFromRaw(event.raw)) {
      const group = grouped.get(image.eventId) ?? [];
      group.push(image);
      grouped.set(image.eventId, group);
    }
    for (const [id, group] of grouped) byEvent.set(id, group);
  }
  return [...byEvent.values()].flat().slice(0, ACP_PROJECTED_IMAGE_LIMIT);
}
