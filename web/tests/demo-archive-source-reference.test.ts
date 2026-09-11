import { createHash } from 'node:crypto';
import { describe, expect, it, vi } from 'vitest';
import { createArchiveReader, type ArchiveSession } from '../../marketing/demo/archive';
import { ARCHIVE_FILE_LIMITS } from '../../marketing/demo/archive-files';

function fixture() {
  const session: ArchiveSession = { projectId: 'project', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt',
    outerNodeId: 'outer', outerAttemptId: 'outer-attempt', detail: `assets/${'1'.repeat(64)}.json`, itemCount: 1, recordCount: 1,
    node: { id: 'node', title: 'Source', status: 'completed', outcome: 'success' } };
  const content = 'original historical source\r\n';
  const sha256 = createHash('sha256').update(content).digest('hex');
  const resource = { path: `resources/${sha256}`, sha256, bytes: Buffer.byteLength(content) };
  const { detail: _, node, itemCount, recordCount, ...locator } = session;
  const reference = { branchId: 'root', eventId: 'message', href: '/E:/source/main.ts:3', sourcePath: 'E:/source/main.ts', resource };
  const index = { version: 1, locator, references: [reference] };
  const detail = { snapshot: null, branches: [{ id: 'root', count: 1, pages: [], tools: {} }], diagnostics: {
    rawFrameCount: 0, eventCount: 1, errorCount: 0, lastError: null, lastErrorTimestamp: null }, workerRef: {}, changeSets: {},
    directoryRoot: `assets/${'2'.repeat(64)}.json`, rawFrames: { resource: `resources/${'3'.repeat(64)}`, bytes: 0, count: 0, pages: [] }, sourceReferences: '' };
  const calls: { path: string; signal?: AbortSignal | null }[] = [];
  let indexResponse: () => Response;
  function publishIndex() {
    const bytes = JSON.stringify(index);
    detail.sourceReferences = `assets/${createHash('sha256').update(bytes).digest('hex')}.json`;
    indexResponse = () => new Response(bytes);
  }
  publishIndex();
  const reader = createArchiveReader('/archive', async (input, init) => {
    const path = String(input).replace('/archive/', '');
    calls.push({ path, signal: init?.signal });
    if (init?.signal?.aborted) throw new DOMException('Aborted', 'AbortError');
    if (path === session.detail) return Response.json(detail);
    if (path === detail.sourceReferences) return indexResponse();
    if (path === resource.path) return new Response(content);
    return new Response('', { status: 404 });
  });
  return { session, reader, index, detail, calls, content, reference, resource, publishIndex,
    setIndexResponse: (response: () => Response) => { indexResponse = response; } };
}

describe('scoped historical source reference reading', () => {
  it('loads one session index on demand and leaves the source body for a separate read', async () => {
    const f = fixture();
    const result = await f.reader.sourceReference(f.session, 'root', 'message', f.reference.href);
    expect(result).toEqual(f.reference);
    expect(f.calls.map(call => call.path)).toEqual([f.session.detail, f.detail.sourceReferences]);
    expect(await f.reader.resourceText(result.resource)).toBe(f.content);
    expect(f.calls.at(-1)?.path).toBe(f.resource.path);
  });
  it('verifies archive digests on plain-HTTP origins without crypto.subtle', async () => {
    const f = fixture();
    const source = globalThis.crypto;
    vi.stubGlobal('crypto', { getRandomValues: source.getRandomValues.bind(source) });
    try {
      expect(globalThis.crypto.subtle).toBeUndefined();
      // Content addressing and the resource digest both have to agree with the native hash above.
      const result = await f.reader.sourceReference(f.session, 'root', 'message', f.reference.href);
      expect(result).toEqual(f.reference);
      expect(await f.reader.resourceText(result.resource)).toBe(f.content);
      await expect(f.reader.resourceText({ ...f.resource, sha256: 'f'.repeat(64) }))
        .rejects.toEqual({ code: 'demo.archive-resource-corrupt', params: {} });
    } finally { vi.unstubAllGlobals(); }
  });
  it('rejects other projects, nested attempts, branches, events and hrefs', async () => {
    const f = fixture();
    for (const field of ['projectId', 'taskId', 'runId', 'roundId', 'nodeId', 'attemptId', 'outerNodeId', 'outerAttemptId'] as const) {
      await expect(f.reader.sourceReference({ ...f.session, [field]: 'other' }, 'root', 'message', f.reference.href)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    }
    for (const [branch, event, href] of [['other', 'message', f.reference.href], ['root', 'other', f.reference.href], ['root', 'message', '/E:/source/other.ts:3']]) {
      await expect(f.reader.sourceReference(f.session, branch, event, href)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    }
    expect(f.calls.some(call => call.path.startsWith('resources/'))).toBe(false);
  });
  it('rejects tampered indices and ambiguous mappings', async () => {
    const f = fixture();
    f.setIndexResponse(() => new Response(JSON.stringify({ ...f.index, references: [] })));
    await expect(f.reader.sourceReference(f.session, 'root', 'message', f.reference.href)).rejects.toMatchObject({ code: 'demo.archive-resource-corrupt' });
    f.index.references.push({ ...f.reference });
    f.publishIndex();
    await expect(f.reader.sourceReference(f.session, 'root', 'message', f.reference.href)).rejects.toMatchObject({ code: 'demo.archive-resource-corrupt' });
  });
  it('propagates cancellation and cancels oversized index streams', async () => {
    const f = fixture();
    const controller = new AbortController();
    controller.abort();
    await expect(f.reader.sourceReference(f.session, 'root', 'message', f.reference.href, controller.signal)).rejects.toMatchObject({ name: 'AbortError' });
    expect(f.calls[0].signal).toBe(controller.signal);
    let cancelled = false;
    f.setIndexResponse(() => new Response(new ReadableStream({ start(stream) {
      stream.enqueue(new Uint8Array(ARCHIVE_FILE_LIMITS.textBytes + 1));
    }, cancel() { cancelled = true; } })));
    await expect(f.reader.sourceReference(f.session, 'root', 'message', f.reference.href)).rejects.toMatchObject({ code: 'demo.archive-range-invalid' });
    expect(cancelled).toBe(true);
  });
});
