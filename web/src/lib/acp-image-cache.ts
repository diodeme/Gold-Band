import { getAcpImage } from '@/api';
import type { AcpImageRef, TurnFileLocatorVm } from '@/types';

export const ACP_IMAGE_CACHE_ENTRIES = 80;
export const ACP_IMAGE_CACHE_BYTES = 48 * 1024 * 1024;
export const ACP_PROJECTED_IMAGE_LIMIT = 256;
const IMAGE_REQUEST_CONCURRENCY = 2;

export function acpImageKey(locator: TurnFileLocatorVm, image: AcpImageRef, thumbnail: boolean) {
  return JSON.stringify([locator.projectId, locator.taskId, locator.runId, locator.roundId, locator.nodeId,
    locator.attemptId, locator.outerNodeId, locator.outerAttemptId, locator.branchId,
    image.eventId, image.pointer, image.contentHash, thumbnail]);
}

export interface AcpImageAsset { url: string; blob: Blob; mimeType: string }
interface Entry { refs: number; asset?: AcpImageAsset; promise: Promise<AcpImageAsset> }

// Shared leases keep both thumbnail entrances on the same URL. Only unmounted
// consumers are evictable; admission and request concurrency are bounded too.
export class AcpImageCache {
  private entries = new Map<string, Entry>();
  private bytes = 0;
  private active = 0;
  private waiting: Array<() => void> = [];

  constructor(private readonly maxEntries = ACP_IMAGE_CACHE_ENTRIES, private readonly maxBytes = ACP_IMAGE_CACHE_BYTES) {}

  private remove(key: string, entry: Entry) {
    if (this.entries.get(key) !== entry) return;
    this.entries.delete(key);
    if (entry.asset) {
      this.bytes -= entry.asset.blob.size;
      URL.revokeObjectURL(entry.asset.url);
    }
  }

  private evict(extraBytes: number, extraEntries: number) {
    for (const [key, entry] of this.entries) {
      if (this.bytes + extraBytes <= this.maxBytes && this.entries.size + extraEntries <= this.maxEntries) break;
      if (entry.refs === 0 && entry.asset) this.remove(key, entry);
    }
    return this.bytes + extraBytes <= this.maxBytes && this.entries.size + extraEntries <= this.maxEntries;
  }

  acquire(key: string, load: () => Promise<Blob>) {
    let entry = this.entries.get(key);
    if (!entry) {
      if (!this.evict(0, 1)) throw { code: 'acp.image-cache-full', params: {} };
      entry = { refs: 0, promise: Promise.resolve(null as unknown as AcpImageAsset) };
      const created = entry;
      this.entries.set(key, created);
      created.promise = this.run(async () => {
        if (created.refs === 0) throw { code: 'acp.image-cancelled', params: {} };
        const blob = await load();
        if (!this.evict(blob.size, 0)) throw { code: 'acp.image-cache-full', params: {} };
        const asset = { blob, url: URL.createObjectURL(blob), mimeType: blob.type };
        created.asset = asset;
        this.bytes += blob.size;
        return asset;
      }).catch((error) => { this.remove(key, created); throw error; });
    }
    this.entries.delete(key);
    this.entries.set(key, entry);
    entry.refs += 1;
    const leased = entry;
    let released = false;
    return { promise: leased.promise, release: () => {
      if (released) return;
      released = true;
      leased.refs -= 1;
    } };
  }

  private async run<T>(operation: () => Promise<T>): Promise<T> {
    if (this.active >= IMAGE_REQUEST_CONCURRENCY) await new Promise<void>((resolve) => this.waiting.push(resolve));
    else this.active += 1;
    try { await Promise.resolve(); return await operation(); }
    finally {
      const next = this.waiting.shift();
      if (next) next(); else this.active -= 1;
    }
  }

  clearUnused() {
    for (const [key, entry] of this.entries) if (entry.refs === 0 && entry.asset) this.remove(key, entry);
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
