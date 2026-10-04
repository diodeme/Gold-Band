export interface LeasedBlobCacheError {
  code: string;
  params: Record<string, unknown>;
}

export interface LeasedBlobAsset<TMeta> {
  url: string;
  blob: Blob;
  mimeType: string;
  meta: TMeta;
}

export interface LeasedBlob<TMeta> {
  blob: Blob;
  meta: TMeta;
}

export interface LeasedBlobLease<TMeta> {
  promise: Promise<LeasedBlobAsset<TMeta>>;
  release: () => void;
}

export interface LeasedBlobCacheOptions {
  maxEntries: number;
  maxBytes: number;
  concurrency: number;
  errors: { full: LeasedBlobCacheError; cancelled: LeasedBlobCacheError };
}

interface Entry<TMeta> {
  refs: number;
  asset?: LeasedBlobAsset<TMeta>;
  promise: Promise<LeasedBlobAsset<TMeta>>;
}

// Shared leases keep every consumer of one key on the same object URL. Only
// entries without a mounted consumer are evictable; admission and load
// concurrency are bounded too.
export class LeasedBlobCache<TMeta> {
  private entries = new Map<string, Entry<TMeta>>();
  private bytes = 0;
  private active = 0;
  private waiting: Array<() => void> = [];

  constructor(private readonly options: LeasedBlobCacheOptions) {}

  private remove(key: string, entry: Entry<TMeta>) {
    if (this.entries.get(key) !== entry) return;
    this.entries.delete(key);
    if (entry.asset) {
      this.bytes -= entry.asset.blob.size;
      URL.revokeObjectURL(entry.asset.url);
    }
  }

  private fits(extraBytes: number, extraEntries: number) {
    return this.bytes + extraBytes <= this.options.maxBytes
      && this.entries.size + extraEntries <= this.options.maxEntries;
  }

  private evict(extraBytes: number, extraEntries: number) {
    for (const [key, entry] of this.entries) {
      if (this.fits(extraBytes, extraEntries)) break;
      if (entry.refs === 0 && entry.asset) this.remove(key, entry);
    }
    return this.fits(extraBytes, extraEntries);
  }

  /** Ready asset for `key` without leasing it, so a remount can render its final size synchronously. */
  peek(key: string) {
    return this.entries.get(key)?.asset ?? null;
  }

  acquire(key: string, load: () => Promise<LeasedBlob<TMeta>>): LeasedBlobLease<TMeta> {
    let entry = this.entries.get(key);
    if (!entry) {
      if (!this.evict(0, 1)) throw this.options.errors.full;
      entry = { refs: 0, promise: Promise.resolve(null as unknown as LeasedBlobAsset<TMeta>) };
      const created = entry;
      this.entries.set(key, created);
      created.promise = this.run(async () => {
        if (created.refs === 0) throw this.options.errors.cancelled;
        const { blob, meta } = await load();
        if (!this.evict(blob.size, 0)) throw this.options.errors.full;
        const asset = { blob, url: URL.createObjectURL(blob), mimeType: blob.type, meta };
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
    if (this.active >= this.options.concurrency) await new Promise<void>((resolve) => this.waiting.push(resolve));
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
