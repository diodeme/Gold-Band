import { describe, expect, it } from 'vitest';
import { archiveSession, createArchiveReader, type ArchiveCatalog } from '../../marketing/demo/archive';

const path = (n: number) => `assets/${String(n).padStart(64, '0')}.json`;
const locator = { projectId: 'p', taskId: 't', runId: 'r', roundId: 'round', nodeId: 'child', attemptId: 'a', outerNodeId: 'outer', outerAttemptId: 'oa' };
const session = { ...locator, node: { id: 'child', title: 'Original title', status: 'paused', outcome: null }, detail: path(1), itemCount: 200, recordCount: 300 };
const catalog: ArchiveCatalog = { version: 1, runtimeVersion: 1, projectId: 'p', task: { id: 't', title: 'Original task' }, run: { id: 'r', status: 'paused', outcome: null }, workflow: path(9), runView: path(10), sessions: [session] };
function fixture() {
  const events = Array.from({ length: 200 }, (_, index) => ({ id: `event-${index + 1}`, seq: index + 1, kind: 'textDelta', timestamp: '2026-09-01T00:00:00Z', content: `Original ${index + 1}` }));
  const pages = [events.slice(0, 96), events.slice(96, 192), events.slice(192)];
  const resources: Record<string, unknown> = {
    'catalog.json': catalog,
    [path(1)]: { directoryRoot: path(8), changeSets: {}, snapshot: { latestTurnStatus: 'paused' }, diagnostics: { rawFrameCount: 0, eventCount: events.length, errorCount: 0, lastError: null, lastErrorTimestamp: null },
      workerRef: { provider: 'codex-acp' }, rawFrames: { resource: `resources/${'0'.repeat(64)}`, bytes: 0, count: 0, pages: [] },
      branches: [{ id: 'root', count: events.length, tools: { tool: path(5) }, pages: pages.map((items, index) => ({ path: path(index + 2), count: items.length, firstSeq: items[0].seq, lastSeq: items.at(-1)!.seq })) }] },
    [path(5)]: { ...events[0], id: 'tool', kind: 'toolCall', raw: { output: 'Full output' } },
  };
  pages.forEach((items, index) => { resources[path(index + 2)] = items; });
  const calls: string[] = [];
  const fetcher = async (input: RequestInfo | URL, init?: RequestInit) => {
    init?.signal?.throwIfAborted();
    const key = String(input).replace('/archive/', '');
    calls.push(key);
    return new Response(JSON.stringify(resources[key]), { status: resources[key] ? 200 : 404 });
  };
  return { reader: createArchiveReader('/archive', fetcher as typeof fetch), calls };
}

describe('static archive interface', () => {
  it('normalizes network failures without exposing browser exception text and preserves cancellation', async () => {
    const reader = createArchiveReader('/archive', async () => { throw new TypeError('Failed to fetch'); });
    await expect(reader.catalog()).rejects.toEqual({ code: 'demo.archive-read-failed', params: {} });
    await expect(reader.resourceBytes({ path: `resources/${'a'.repeat(64)}`, sha256: 'a'.repeat(64), bytes: 12 }))
      .rejects.toEqual({ code: 'demo.archive-read-failed', params: {} });
    const cancelled = new DOMException('Cancelled', 'AbortError');
    const abortReader = createArchiveReader('/archive', async () => { throw cancelled; });
    await expect(abortReader.catalog()).rejects.toBe(cancelled);
  });
  function rawFixture(mode: 'valid' | 'full' | 'short' | 'long' = 'valid') {
    const encoder = new TextEncoder();
    const lines = Array.from({ length: 201 }, (_, i) => JSON.stringify({ timestamp: `${i}Z`, direction: 'outbound', frame: { method: 'session/update', params: { text: `原始正文 ${i + 1}` } } }) + '\n');
    const body = encoder.encode(lines.join(''));
    let offset = 0;
    const pages = [0, 96, 192].map(start => {
      const length = encoder.encode(lines.slice(start, start + 96).join('')).length;
      const part = { offset, length, count: Math.min(96, lines.length - start), firstLine: start + 1 };
      offset += length;
      return part;
    });
    const ranges: string[] = [];
    const fetcher: typeof fetch = async (input, init) => {
      init?.signal?.throwIfAborted();
      if (String(input).endsWith(session.detail)) return Response.json({ directoryRoot: path(8), changeSets: {}, snapshot: null, branches: [], workerRef: {},
        diagnostics: { rawFrameCount: 201, eventCount: 0, errorCount: 0, lastError: null, lastErrorTimestamp: null },
        rawFrames: { resource: `resources/${'0'.repeat(64)}`, bytes: body.length, count: 201, pages } });
      const range = new Headers(init?.headers).get('Range')!;
      ranges.push(range);
      const [, first, last] = /^bytes=(\d+)-(\d+)$/.exec(range)!;
      const bytes = body.slice(Number(first), Number(last) + 1);
      return new Response(mode === 'short' ? bytes.slice(1) : mode === 'long' ? new Uint8Array(bytes.length + 1) : bytes, {
        status: mode === 'full' ? 200 : 206,
        headers: { 'Content-Range': `bytes ${first}-${last}/${body.length}` },
      });
    };
    return { reader: createArchiveReader('/archive', fetcher), ranges, lines };
  }
  it('reads raw frames in both directions across UTF-8 byte pages without omission', async () => {
    for (const order of ['asc', 'desc'] as const) {
      const { reader, lines } = rawFixture();
      const all = [];
      for (let page = 0; page < 5; page++) {
        const result = await reader.rawPage(session, page, 50, order);
        expect(result.total).toBe(201);
        expect(result.hasNext).toBe(page < 4);
        all.push(...result.items);
      }
      const numbers = Array.from({ length: 201 }, (_, i) => i + 1);
      expect(all.map(item => item.lineNumber)).toEqual(order === 'asc' ? numbers : numbers.reverse());
      for (const item of all) expect(item.content).toBe(lines[item.lineNumber - 1].trimEnd());
    }
  });
  it('rejects full-body responses and incorrect byte lengths', async () => {
    await expect(rawFixture('full').reader.rawPage(session)).rejects.toMatchObject({ code: 'demo.archive-range-unsupported' });
    for (const mode of ['short', 'long'] as const) {
      await expect(rawFixture(mode).reader.rawPage(session)).rejects.toMatchObject({ code: 'demo.archive-range-invalid' });
    }
  });
  it('joins storage chunks for a 200-row UI page', async () => {
    const { reader } = rawFixture();
    expect((await reader.rawPage(session, 0, 200, 'asc')).items.map(item => item.lineNumber))
      .toEqual(Array.from({ length: 200 }, (_, i) => i + 1));
    expect((await reader.rawPage(session, 1, 200, 'asc')).items.map(item => item.lineNumber)).toEqual([201]);
  });
  it('does not request raw content for empty pages, invalid cursors or cancelled reads', async () => {
    const { reader, ranges } = rawFixture();
    expect((await reader.rawPage(session, 100)).items).toEqual([]);
    await expect(reader.rawPage(session, -1)).rejects.toMatchObject({ code: 'demo.archive-cursor-invalid' });
    await expect(reader.rawPage(session, 0, 201)).rejects.toMatchObject({ code: 'demo.archive-cursor-invalid' });
    const controller = new AbortController();
    controller.abort();
    await expect(reader.rawPage(session, 0, 96, 'desc', controller.signal)).rejects.toMatchObject({ name: 'AbortError' });
    expect(ranges).toEqual([]);
  });
  it('isolates the complete locator, including project and outer attempt', () => {
    expect(archiveSession(catalog, locator)).toBe(session);
    for (const key of Object.keys(locator)) {
      expect(() => archiveSession(catalog, { ...locator, [key]: 'another' })).toThrow();
    }
    expect(() => archiveSession(catalog, { ...locator, outerAttemptId: undefined })).toThrow();
  });
  it('loads only the catalog before a session is requested', async () => {
    const { reader, calls } = fixture();
    expect((await reader.catalog()).task.title).toBe('Original task');
    expect(calls).toEqual(['catalog.json']);
  });
  it('reads all original items backward across partial static pages without gaps or duplicates', async () => {
    const { reader, calls } = fixture();
    const latest = await reader.history(session, { pageSize: 50 });
    expect(latest.events.map(event => event.seq)).toEqual(Array.from({ length: 50 }, (_, i) => 151 + i));
    expect(latest.eventPage.hasOlder).toBe(true);
    expect(latest.eventPage.hasNewer).toBe(false);
    expect(calls).toEqual([path(1), path(4), path(3)]);
    let page = latest;
    let all = page.events;
    while (page.eventPage.hasOlder) {
      page = await reader.history(session, { beforeCursor: page.eventPage.oldestCursor!, pageSize: 50 });
      all = [...page.events, ...all];
    }
    expect(all.map(event => event.seq)).toEqual(Array.from({ length: 200 }, (_, i) => i + 1));
    expect(all[0].content).toBe('Original 1');
    expect(new Set(all.map(event => event.id)).size).toBe(200);
    expect(calls).not.toContain(path(5));
  });
  it('supports forward reads, rejects unknown branches and malformed cursors, and loads tool bodies on demand', async () => {
    const { reader } = fixture();
    const page = await reader.history(session, { afterSeq: 90, pageSize: 20 });
    expect(page.events.map(event => event.seq)).toEqual(Array.from({ length: 20 }, (_, i) => 91 + i));
    await expect(reader.history(session, { branchId: 'missing' })).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(reader.history(session, { beforeCursor: 'not-a-sequence' })).rejects.toMatchObject({ code: 'demo.archive-cursor-invalid' });
    expect((await reader.tool(session, 'root', 'tool')).raw).toEqual({ output: 'Full output' });
    await expect(reader.tool(session, 'root', 'unknown')).rejects.toMatchObject({ code: 'demo.resource-not-found' });
  });
  it('propagates cancellation without supplying unrelated data', async () => {
    const { reader, calls } = fixture();
    const controller = new AbortController();
    controller.abort();
    await expect(reader.history(session, {}, controller.signal)).rejects.toMatchObject({ name: 'AbortError' });
    expect(calls).toEqual([]);
  });
});
