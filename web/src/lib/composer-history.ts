export interface ComposerHistoryLocator {
  projectId: string;
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
}

export interface HistoryCursor { generation: number; messageId: string; position: number }
export type HistoryDirection = 'older' | 'newer';
export interface HistoryQuery {
  cursor?: HistoryCursor | null;
  head?: HistoryCursor | null;
  direction: HistoryDirection;
  limit: number;
}
export interface HistorySummary { cursor: HistoryCursor; textBytes: number }
export interface HistoryPage { items: HistorySummary[]; head: HistoryCursor | null; nextCursor: HistoryCursor | null }
export interface HistoryText { cursor: HistoryCursor; text: string }
export interface ComposerHistorySource {
  list: (query: HistoryQuery) => Promise<HistoryPage>;
  text: (cursor: HistoryCursor) => Promise<HistoryText>;
}

export const COMPOSER_HISTORY_LIMITS = { pageSize: 20, entries: 40, textBytes: 1024 * 1024, textEntries: 3 } as const;

/** Disposable read projection; persisted messages and the composer draft remain authoritative. */
export class ComposerHistoryReader {
  private entries: HistorySummary[] = [];
  private texts = new Map<string, string>();
  private head: HistoryCursor | null = null;

  constructor(private readonly source: ComposerHistorySource) {}

  async move(current: HistoryCursor | null, direction: HistoryDirection, active: () => boolean): Promise<HistoryText | null> {
    const index = current ? this.entries.findIndex((entry) => entry.cursor.messageId === current.messageId) : -1;
    const neighbor = index < 0 ? undefined : this.entries[index + (direction === 'older' ? 1 : -1)];
    if (neighbor) return this.readText(neighbor.cursor, active);
    let bound = current;
    let wrapped = false;
    while (active()) {
      const page = await this.source.list({ cursor: bound, head: this.head, direction, limit: COMPOSER_HISTORY_LIMITS.pageSize });
      if (!active()) return null;
      this.head = page.head;
      if (page.items.length) {
        const combined = new Map(this.entries.map((entry) => [entry.cursor.messageId, entry]));
        page.items.forEach((entry) => combined.set(entry.cursor.messageId, entry));
        const ordered = [...combined.values()].sort((a, b) => b.cursor.position - a.cursor.position || b.cursor.messageId.localeCompare(a.cursor.messageId));
        this.entries = direction === 'older' && !wrapped
          ? ordered.slice(-COMPOSER_HISTORY_LIMITS.entries) : ordered.slice(0, COMPOSER_HISTORY_LIMITS.entries);
        return this.readText(page.items[0].cursor, active);
      }
      if (page.nextCursor) { bound = page.nextCursor; continue; }
      if (direction === 'older' && current && !wrapped) {
        this.entries = [];
        bound = null;
        wrapped = true;
        continue;
      }
      return null;
    }
    return null;
  }

  private async readText(cursor: HistoryCursor, active: () => boolean): Promise<HistoryText | null> {
    const cached = this.texts.get(cursor.messageId);
    if (cached !== undefined) return { cursor, text: cached };
    const result = await this.source.text(cursor);
    if (!active()) return null;
    if (result.cursor.generation !== cursor.generation || result.cursor.messageId !== cursor.messageId || result.cursor.position !== cursor.position) {
      throw { code: 'acp.composer-history-stale', params: {} };
    }
    // UTF-16 storage estimate avoids allocating encoded copies on every navigation.
    if (result.text.length * 2 <= COMPOSER_HISTORY_LIMITS.textBytes) {
      this.texts.set(cursor.messageId, result.text);
      while (this.texts.size > COMPOSER_HISTORY_LIMITS.textEntries
        || [...this.texts.values()].reduce((sum, text) => sum + text.length * 2, 0) > COMPOSER_HISTORY_LIMITS.textBytes) {
        this.texts.delete(this.texts.keys().next().value!);
      }
    }
    return result;
  }
}
