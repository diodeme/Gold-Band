import { getRuntimeApi } from '@/api/client';
import type { BrowserBookmark } from './browser-bookmarks';

type Listener = () => void;

export class BrowserBookmarkStore {
  private items: BrowserBookmark[] = [];
  private readonly listeners = new Set<Listener>();
  private started = false;
  private unlisten: (() => void) | null = null;
  private generation = 0;

  snapshot() {
    return this.items;
  }

  subscribe(listener: Listener) {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  async refresh() {
    const generation = ++this.generation;
    await this.ensureEvents();
    const items = await getRuntimeApi().browserListBookmarks();
    if (generation !== this.generation) return;
    this.items = items;
    this.emit();
  }

  async add(url: string, title?: string) {
    const items = await getRuntimeApi().browserAddBookmark({ url, title });
    this.items = items;
    this.emit();
    return items;
  }

  async remove(bookmarkId: string) {
    const items = await getRuntimeApi().browserRemoveBookmark({ bookmarkId });
    this.items = items;
    this.emit();
    return items;
  }

  async reorder(orderedIds: string[]) {
    const items = await getRuntimeApi().browserReorderBookmarks({ orderedIds });
    this.items = items;
    this.emit();
    return items;
  }

  resetForTests() {
    this.unlisten?.();
    this.unlisten = null;
    this.started = false;
    this.generation += 1;
    this.items = [];
    this.emit();
  }

  replaceForTests(items: BrowserBookmark[]) {
    this.items = items;
    this.emit();
  }

  private async ensureEvents() {
    if (this.started) return;
    this.started = true;
    this.unlisten = await getRuntimeApi().subscribeBrowserHistoryEvents?.(() => {
      void this.refresh();
    }) ?? null;
  }

  private emit() {
    for (const listener of this.listeners) listener();
  }
}

export const browserBookmarkStore = new BrowserBookmarkStore();
