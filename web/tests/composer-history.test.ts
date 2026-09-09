import { describe, expect, it, vi } from 'vitest';
import { ComposerHistoryReader, type ComposerHistorySource, type HistoryCursor } from '@/lib/composer-history';

function historySource(count: number, contents?: string[]): ComposerHistorySource {
  const entries = Array.from({ length: count }, (_, index) => ({
    cursor: { generation: 1, messageId: `message-${index + 1}`, position: index + 1 }, textBytes: 10,
  })).reverse();
  return {
    list: vi.fn(async ({ cursor, head, direction, limit }) => {
      const items = entries.filter((entry) => (!head || entry.cursor.position <= head.position)
        && (!cursor || (direction === 'older' ? entry.cursor.position < cursor.position : entry.cursor.position > cursor.position)));
      if (direction === 'newer') items.reverse();
      return { items: items.slice(0, limit), head: head ?? entries[0]?.cursor ?? null,
        nextCursor: items.length > limit ? items[limit - 1].cursor : null };
    }),
    text: vi.fn(async (cursor) => ({ cursor, text: contents?.[cursor.position - 1] ?? `hello${cursor.position}` })),
  };
}

describe('composer history read interface', () => {
  it('keeps the earliest persisted input reachable after an hour and after cache recreation', async () => {
    vi.useFakeTimers();
    try {
      const source = historySource(81);
      let reader = new ComposerHistoryReader(source);
      let entry = await reader.move(null, 'older', () => true);
      vi.advanceTimersByTime(60 * 60 * 1000);
      for (let index = 80; index > 0; index -= 1) entry = await reader.move(entry!.cursor, 'older', () => true);
      expect(entry?.text).toBe('hello1');
      reader = new ComposerHistoryReader(source);
      entry = await reader.move(null, 'older', () => true);
      for (let index = 80; index > 0; index -= 1) entry = await reader.move(entry!.cursor, 'older', () => true);
      expect(entry?.text).toBe('hello1');
    } finally { vi.useRealTimers(); }
  });
  it('crosses cache eviction boundaries and wraps without skipping the middle', async () => {
    const source = historySource(101);
    const reader = new ComposerHistoryReader(source);
    let cursor: HistoryCursor | null = null;
    for (let lap = 0; lap < 2; lap += 1) {
      for (let position = 101; position > 0; position -= 1) {
        const result = await reader.move(cursor, 'older', () => true);
        expect(result?.text).toBe(`hello${position}`);
        cursor = result!.cursor;
      }
    }
    for (let position = 2; position <= 101; position += 1) {
      const result = await reader.move(cursor, 'newer', () => true);
      expect(result?.text).toBe(`hello${position}`);
      cursor = result!.cursor;
    }
    expect(await reader.move(cursor, 'newer', () => true)).toBeNull();
    expect(vi.mocked(source.list).mock.calls.length).toBeLessThan(25);
  });

  it('retains identical submissions by identity and handles one or zero entries', async () => {
    const reader = new ComposerHistoryReader(historySource(2, ['same', 'same']));
    const first = await reader.move(null, 'older', () => true);
    const second = await reader.move(first!.cursor, 'older', () => true);
    expect(first?.text).toBe(second?.text);
    expect(first?.cursor.messageId).not.toBe(second?.cursor.messageId);
    const single = new ComposerHistoryReader(historySource(1));
    const only = await single.move(null, 'older', () => true);
    expect(await single.move(only!.cursor, 'older', () => true)).toEqual(only);
    expect(await new ComposerHistoryReader(historySource(0)).move(null, 'older', () => true)).toBeNull();
  });

  it('does not read text after cancellation and traverses empty candidate pages', async () => {
    const source = historySource(1);
    vi.mocked(source.list).mockImplementationOnce(async () => ({ items: [], head: null,
      nextCursor: { generation: 1, messageId: 'empty', position: 2 } }));
    const reader = new ComposerHistoryReader(source);
    expect((await reader.move(null, 'older', () => true))?.text).toBe('hello1');
    expect(source.list).toHaveBeenCalledTimes(2);
    const cancelled = historySource(1);
    let active = true;
    vi.mocked(cancelled.list).mockImplementationOnce(async () => { active = false; return { items: [], head: null, nextCursor: null }; });
    expect(await new ComposerHistoryReader(cancelled).move(null, 'older', () => active)).toBeNull();
    expect(cancelled.text).not.toHaveBeenCalled();
  });
});
