import { getRuntimeApi } from '@/api/client';
import type { BrowserVisit } from './browser-history';

type Listener = () => void;

export class BrowserHistoryStore {
  private visits: BrowserVisit[] = [];
  private readonly listeners = new Set<Listener>();
  private started = false;
  private unlisten: (() => void) | null = null;
  private generation = 0;

  snapshot() {
    return this.visits;
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
    const visits = await getRuntimeApi().browserListHistory();
    if (generation !== this.generation) return;
    this.visits = visits;
    this.emit();
  }

  async remove(url: string) {
    const generation = ++this.generation;
    await this.ensureEvents();
    const visits = await getRuntimeApi().browserDeleteHistory({ url });
    if (generation !== this.generation) return;
    this.visits = visits;
    this.emit();
  }

  resetForTests() {
    this.unlisten?.();
    this.unlisten = null;
    this.started = false;
    this.generation += 1;
    this.visits = [];
    this.emit();
  }

  replaceForTests(visits: BrowserVisit[]) {
    this.visits = visits;
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

export const browserHistoryStore = new BrowserHistoryStore();
