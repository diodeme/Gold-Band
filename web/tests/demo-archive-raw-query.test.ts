import { describe, expect, it, vi, afterEach } from 'vitest';
import { normalizeRawQuery, queryArchiveRawFrames } from '../../marketing/demo/archive-raw-query';
import { runArchiveRawQuery } from '../../marketing/demo/archive-raw-client';

function fixture() {
  const lines = Array.from({ length: 201 }, (_, i) => JSON.stringify({ timestamp: `${i}Z`, direction: i % 2 ? 'in' : 'out',
    frame: { method: i % 3 ? 'session/update' : 'session/prompt', params: { content: `原始 ${i} Keep` } } }));
  const bytes = new TextEncoder().encode(lines.join('\n') + '\n');
  const pages = [];
  let offset = 0;
  for (let i = 0; i < lines.length; i += 96) {
    const chunk = lines.slice(i, i + 96);
    const length = new TextEncoder().encode(chunk.join('\n') + '\n').length;
    pages.push({ offset, length, firstLine: i + 1, count: chunk.length });
    offset += length;
  }
  const calls: string[] = [];
  const fetcher = vi.fn<typeof fetch>(async (_, init) => {
    init?.signal?.throwIfAborted();
    const range = new Headers(init?.headers).get('Range')!;
    calls.push(range);
    const [from, to] = range.slice(6).split('-').map(Number);
    return new Response(bytes.slice(from, to + 1), { status: 206, headers: { 'Content-Range': `bytes ${from}-${to}/${bytes.length}` } });
  });
  return { index: { resource: `resources/${'a'.repeat(64)}`, bytes: bytes.length, count: lines.length, pages }, fetcher, calls, lines };
}

describe('archive raw filtering', () => {
  it('matches normalized source filters and pages without missing or duplicating original rows', async () => {
    const { index, fetcher, lines, calls } = fixture();
    const query = { search: ' KEEP ', kind: ' UPDATE ', direction: ' IN ', pageSize: 17, order: 'asc' as const };
    const expected = lines.flatMap((line, i) => i % 2 && i % 3 ? [i + 1] : []);
    const all = [];
    for (let page = 0; page < Math.ceil(expected.length / 17); page++) {
      const result = await queryArchiveRawFrames('/raw', index, { ...query, page }, fetcher);
      expect(result.total).toBe(expected.length);
      expect(result).toMatchObject({ search: 'keep', kind: 'update', direction: 'in' });
      expect(result.items.every(item => item.content === lines[item.lineNumber - 1])).toBe(true);
      all.push(...result.items.map(item => item.lineNumber));
    }
    expect(all).toEqual(expected);
    expect(calls.length).toBe(12);
    const descending = await queryArchiveRawFrames('/raw', index, { ...query, order: 'desc' }, fetcher);
    expect(descending.items.map(item => item.lineNumber)).toEqual(expected.toReversed().slice(0, 17));
    const empty = await queryArchiveRawFrames('/raw', index, { search: 'not present', page: 8 }, fetcher);
    expect(empty).toMatchObject({ items: [], total: 0, hasPrevious: false, hasNext: false });
    expect(normalizeRawQuery({ search: '   ' }).search).toBeNull();
    const full = await queryArchiveRawFrames('/raw', index, { search: 'keep', pageSize: 200, order: 'asc' }, fetcher);
    expect(full.items.map(item => item.lineNumber)).toEqual(Array.from({ length: 200 }, (_, i) => i + 1));
    expect(full.hasNext).toBe(true);
    expect((await queryArchiveRawFrames('/raw', index, { search: 'keep', pageSize: 200, order: 'asc', page: 1 }, fetcher)).items.map(item => item.lineNumber)).toEqual([201]);
  });
  it('rejects unsupported ranges, corrupted lengths and aborted scans', async () => {
    const { index, fetcher } = fixture();
    await expect(queryArchiveRawFrames('/raw', index, {}, async () => new Response('full'))).rejects.toMatchObject({ code: 'demo.archive-range-unsupported' });
    const damaged = vi.fn<typeof fetch>(async (input, init) => {
      const response = await fetcher(input, init);
      return new Response('short', { status: 206, headers: response.headers });
    });
    await expect(queryArchiveRawFrames('/raw', index, {}, damaged)).rejects.toMatchObject({ code: 'demo.archive-range-invalid' });
    const controller = new AbortController();
    const cancelling = vi.fn<typeof fetch>(async (input, init) => { const response = await fetcher(input, init); controller.abort(); return response; });
    await expect(queryArchiveRawFrames('/raw', index, {}, cancelling, controller.signal)).rejects.toMatchObject({ name: 'AbortError' });
    expect(cancelling).toHaveBeenCalledTimes(1);
    const lastController = new AbortController();
    let requests = 0;
    const cancelLast = vi.fn<typeof fetch>(async (input, init) => {
      const response = await fetcher(input, init);
      if (++requests === index.pages.length) lastController.abort();
      return response;
    });
    await expect(queryArchiveRawFrames('/raw', index, {}, cancelLast, lastController.signal)).rejects.toMatchObject({ name: 'AbortError' });
  });
});

describe('archive raw worker lifecycle', () => {
  afterEach(() => vi.unstubAllGlobals());
  it('terminates the worker on completion, failure and cancellation', async () => {
    const workers: FakeWorker[] = [];
    class FakeWorker {
      onmessage?: (event: { data: unknown }) => void;
      onerror?: () => void;
      terminate = vi.fn();
      postMessage = vi.fn();
      constructor() { workers.push(this); }
    }
    vi.stubGlobal('Worker', FakeWorker);
    const { index } = fixture();
    const complete = runArchiveRawQuery('/raw', index, {});
    workers[0].onmessage!({ data: { result: { total: 1 } } });
    await expect(complete).resolves.toMatchObject({ total: 1 });
    expect(workers[0].terminate).toHaveBeenCalledOnce();
    const failed = runArchiveRawQuery('/raw', index, {});
    workers[1].onerror!();
    await expect(failed).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    expect(workers[1].terminate).toHaveBeenCalledOnce();
    const controller = new AbortController();
    const cancelled = runArchiveRawQuery('/raw', index, {}, controller.signal);
    controller.abort();
    await expect(cancelled).rejects.toMatchObject({ name: 'AbortError' });
    expect(workers[2].terminate).toHaveBeenCalledOnce();
  });
});
