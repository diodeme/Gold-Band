export class BoundedLruCache<K, V> {
  private readonly entries = new Map<K, V>();

  constructor(private readonly limit: number) {
    if (!Number.isInteger(limit) || limit < 1) {
      throw new Error('BoundedLruCache limit must be a positive integer');
    }
  }

  get size() {
    return this.entries.size;
  }

  peek(key: K) {
    return this.entries.get(key);
  }

  get(key: K) {
    const value = this.entries.get(key);
    if (value === undefined) return undefined;
    this.entries.delete(key);
    this.entries.set(key, value);
    return value;
  }

  set(key: K, value: V) {
    this.entries.delete(key);
    this.entries.set(key, value);
    while (this.entries.size > this.limit) {
      const oldest = this.entries.keys().next().value as K | undefined;
      if (oldest === undefined) break;
      this.entries.delete(oldest);
    }
  }

  delete(key: K) {
    return this.entries.delete(key);
  }

  deleteWhere(predicate: (value: V, key: K) => boolean) {
    for (const [key, value] of this.entries) {
      if (predicate(value, key)) this.entries.delete(key);
    }
  }

  keys() {
    return [...this.entries.keys()];
  }

  clear() {
    this.entries.clear();
  }
}
