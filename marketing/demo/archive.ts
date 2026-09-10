import { z } from 'zod';
import type { AcpActivityDetailQueryInput, AcpRawFrameQueryInput, AcpRawFrameVm, AcpSessionQueryInput, AcpUiEventVm, FileComparisonVm, FileVersionRefVm, TurnFileLocatorVm } from '@/types';
import { boundedBytes } from './archive-http';
import { normalizeRawQuery, rawFrame } from './archive-raw-query';
import { runArchiveRawQuery } from './archive-raw-client';
import { archiveDirectorySchema, normalizeArchiveRelativePath } from './archive-directories';
import { archiveRunSchema } from './archive-run';
import { ARCHIVE_FILE_LIMITS, archiveFilesSchema, archiveResourceSchema, type ArchiveResource } from './archive-files';

const assetPath = z.string().regex(/^assets\/[a-f0-9]{64}\.json$/);
const locatorSchema = z.object({
  projectId: z.string(), taskId: z.string(), runId: z.string(), roundId: z.string(),
  nodeId: z.string(), attemptId: z.string(), outerNodeId: z.string().optional(), outerAttemptId: z.string().optional(),
});
const sessionSchema = locatorSchema.extend({
  node: z.object({ id: z.string(), title: z.string(), status: z.string(), outcome: z.string().nullable() }).passthrough(),
  detail: assetPath, itemCount: z.number().int().nonnegative(), recordCount: z.number().int().nonnegative(),
});
const catalogSchema = z.object({
  version: z.literal(1), runtimeVersion: z.literal(1), projectId: z.string(),
  task: z.object({ id: z.string(), title: z.string() }).passthrough(),
  run: z.object({ id: z.string(), status: z.string(), outcome: z.string().nullable() }).passthrough(),
  workflow: assetPath, runView: assetPath, sessions: z.array(sessionSchema),
});
const branchSchema = z.object({
  id: z.string(), count: z.number().int().nonnegative(),
  pages: z.array(z.object({ path: assetPath, count: z.number().int().positive(), firstSeq: z.number().int(), lastSeq: z.number().int() })),
  tools: z.record(z.string(), assetPath),
});
const diagnosticsSchema = z.object({ rawFrameCount: z.number().int().nonnegative(), eventCount: z.number().int().nonnegative(),
  errorCount: z.number().int().nonnegative(), lastError: z.string().nullable(), lastErrorTimestamp: z.string().nullable() });
const rawSchema = z.object({ resource: z.string().regex(/^resources\/[a-f0-9]{64}$/), bytes: z.number().int().nonnegative(), count: z.number().int().nonnegative(),
  pages: z.array(z.object({ offset: z.number().int().nonnegative(), length: z.number().int().positive(), count: z.number().int().positive(), firstLine: z.number().int().positive() })) });
const detailSchema = z.object({ snapshot: z.record(z.string(), z.unknown()).nullable(), branches: z.array(branchSchema),
  diagnostics: diagnosticsSchema, workerRef: z.record(z.string(), z.unknown()), rawFrames: rawSchema, changeSets: z.record(z.string(), assetPath), directoryRoot: assetPath,
  sourceReferences: assetPath.optional() });
const sourceReferenceIndexSchema = z.object({ version: z.literal(1), locator: locatorSchema, references: z.array(z.object({
  branchId: z.string().min(1), eventId: z.string().min(1), href: z.string().min(1), sourcePath: z.string().min(1), resource: archiveResourceSchema,
})) });
const eventsSchema = z.array(z.object({ id: z.string(), seq: z.number().int(), kind: z.string(), timestamp: z.string() }).passthrough());
export type ArchiveCatalog = z.infer<typeof catalogSchema>;
export type ArchiveSession = z.infer<typeof sessionSchema>;
type ArchiveLocator = Omit<TurnFileLocatorVm, 'branchId'>;
const locatorFields = ['projectId', 'taskId', 'runId', 'roundId', 'nodeId', 'attemptId', 'outerNodeId', 'outerAttemptId'] as const;
const failure = (code: string) => ({ code, params: {} });
export function archiveSession(catalog: ArchiveCatalog, locator: ArchiveLocator) {
  const session = catalog.sessions.find(candidate => locatorFields.every(key => (candidate[key] ?? null) === (locator[key] ?? null)));
  if (!session) throw failure('demo.resource-not-found');
  return session;
}

// The reader owns no growing history cache. The shared conversation UI owns its bounded window.
export function createArchiveReader(baseUrl: string, fetcher: typeof fetch = fetch) {
  const base = baseUrl.endsWith('/') ? baseUrl : `${baseUrl}/`;
  async function sha256(bytes: Uint8Array<ArrayBuffer>) {
    return Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)), byte => byte.toString(16).padStart(2, '0')).join('');
  }
  async function request(url: string, init?: RequestInit) {
    try { return await fetcher(url, init); }
    catch (error) {
      if (init?.signal?.aborted || (error instanceof Error && error.name === 'AbortError')) throw error;
      throw failure('demo.archive-read-failed');
    }
  }
  async function read<T>(path: string, schema: z.ZodType<T>, signal?: AbortSignal): Promise<T> {
    if (path !== 'catalog.json' && !assetPath.safeParse(path).success) throw failure('demo.archive-path-invalid');
    const response = await request(`${base}${path}`, { signal });
    if (!response.ok) throw failure('demo.resource-not-found');
    return schema.parse(await response.json());
  }
  async function files(session: ArchiveSession, branchId: string, changeSetId: string, signal?: AbortSignal) {
    const detail = await read(session.detail, detailSchema, signal);
    const path = detail.changeSets[changeSetId];
    if (!path) throw failure('demo.resource-not-found');
    const result = await read(path, archiveFilesSchema, signal);
    if (result.changeSet.id !== changeSetId || result.changeSet.branchId !== branchId) throw failure('demo.resource-not-found');
    return result;
  }
  function resourceUrl(path: string) {
    if (!/^resources\/[a-f0-9]{64}$/.test(path)) throw failure('demo.archive-path-invalid');
    return `${base}${path}`;
  }
  async function resourceBytes(resource: ArchiveResource, maxBytes = ARCHIVE_FILE_LIMITS.textBytes, signal?: AbortSignal) {
    if (resource.bytes > maxBytes) throw failure('workspace-file.too-large');
    const response = await request(resourceUrl(resource.path), { signal });
    if (!response.ok) throw failure('demo.resource-not-found');
    const bytes = await boundedBytes(response, resource.bytes);
    const digest = await sha256(bytes);
    if (digest !== resource.sha256) throw failure('demo.archive-resource-corrupt');
    return bytes;
  }
  async function resourceText(resource: ArchiveResource, maxBytes = ARCHIVE_FILE_LIMITS.textBytes, signal?: AbortSignal) {
    return new TextDecoder('utf-8', { fatal: true }).decode(await resourceBytes(resource, maxBytes, signal));
  }
  async function directory(session: ArchiveSession, relativePath = '', signal?: AbortSignal) {
    const detail = await read(session.detail, detailSchema, signal);
    let listing = await read(detail.directoryRoot, archiveDirectorySchema, signal);
    const canonicalSegments: string[] = [];
    for (const segment of normalizeArchiveRelativePath(relativePath).split('/').filter(Boolean)) {
      const entries = listing.entries.filter(entry => entry.name.toLowerCase() === segment.toLowerCase());
      if (entries.length !== 1 || entries[0].kind !== 'directory') throw failure('conversation-directory.not-found');
      canonicalSegments.push(entries[0].name);
      listing = await read(entries[0].directory, archiveDirectorySchema, signal);
    }
    return { ...listing, relativePath: canonicalSegments.join('/') };
  }
  async function directoryFile(session: ArchiveSession, relativePath: string, signal?: AbortSignal) {
    const parts = normalizeArchiveRelativePath(relativePath).split('/');
    const name = parts.pop()!;
    const listing = await directory(session, parts.join('/'), signal);
    const entries = listing.entries.filter(entry => entry.name.toLowerCase() === name.toLowerCase());
    if (entries.length !== 1 || entries[0].kind !== 'file') throw failure('conversation-directory.not-found');
    return { ...entries[0], relativePath: [listing.relativePath, entries[0].name].filter(Boolean).join('/') };
  }
  return {
    files, resourceText, resourceBytes, resourceUrl, directory, directoryFile,
    async sourceReference(session: ArchiveSession, branchId: string, eventId: string, href: string, signal?: AbortSignal) {
      const detail = await read(session.detail, detailSchema, signal);
      if (!detail.sourceReferences || !detail.branches.some(branch => branch.id === branchId)) throw failure('demo.resource-not-found');
      const response = await request(`${base}${detail.sourceReferences}`, { signal });
      if (!response.ok) throw failure('demo.resource-not-found');
      const bytes = await boundedBytes(response, ARCHIVE_FILE_LIMITS.textBytes, false);
      if (`assets/${await sha256(bytes)}.json` !== detail.sourceReferences) throw failure('demo.archive-resource-corrupt');
      const index = sourceReferenceIndexSchema.parse(JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes)));
      if (!locatorFields.every(field => (session[field] ?? null) === (index.locator[field] ?? null))) throw failure('demo.resource-not-found');
      const matches = index.references.filter(reference => reference.branchId === branchId && reference.eventId === eventId && reference.href === href);
      if (!matches.length) throw failure('demo.resource-not-found');
      if (matches.length !== 1) throw failure('demo.archive-resource-corrupt');
      return matches[0];
    },
    async rawQuery(session: ArchiveSession, input: AcpRawFrameQueryInput, signal?: AbortSignal) {
      const query = normalizeRawQuery(input);
      const { rawFrames: index } = await read(session.detail, detailSchema, signal);
      if (index.count === 0) return { ...query, items: [], total: 0, hasPrevious: false, hasNext: false };
      return runArchiveRawQuery(new URL(resourceUrl(index.resource), location.href).href, index, input, signal);
    },
    async comparison(session: ArchiveSession, branchId: string, changeSetId: string, changeId: string): Promise<FileComparisonVm> {
      const data = await files(session, branchId, changeSetId);
      const change = data.changeSet.changes.find(change => change.id === changeId);
      if (!change) throw failure('demo.resource-not-found');
      const result = { changeSetId, changeId, path: change.logicalPath, stats: { addedLines: change.addedLines, deletedLines: change.deletedLines }, limitationCode: change.limitationCode };
      if ((change.beforeVersion?.byteLength ?? 0) + (change.afterVersion?.byteLength ?? 0) > ARCHIVE_FILE_LIMITS.diffBytes) return { ...result, limitationCode: 'turn-files.diff-too-large' };
      async function snapshot(version?: FileVersionRefVm | null) {
        if (!version) return null;
        const resource = data.versions[version.contentHash];
        if (!resource) throw failure('turn-files.version-not-found');
        return { version, content: await resourceText(resource, ARCHIVE_FILE_LIMITS.diffBytes) };
      }
      const [before, after] = await Promise.all([snapshot(change.beforeVersion), snapshot(change.afterVersion)]);
      const lines = [before, after].reduce((count, side) => count + (side?.content.split('\n').length ?? 0), 0);
      return lines > ARCHIVE_FILE_LIMITS.diffLines ? { ...result, limitationCode: 'turn-files.diff-too-large' } : { ...result, before, after };
    },
    catalog: (signal?: AbortSignal) => read('catalog.json', catalogSchema, signal),
    run: (catalog: ArchiveCatalog, signal?: AbortSignal) => read(catalog.runView, archiveRunSchema, signal),
    detail: (session: ArchiveSession, signal?: AbortSignal) => read(session.detail, detailSchema, signal),
    async history(session: ArchiveSession, query: AcpSessionQueryInput = {}, signal?: AbortSignal) {
      const detail = await read(session.detail, detailSchema, signal);
      const branch = detail.branches.find(branch => branch.id === (query.branchId ?? 'root'));
      if (!branch) throw failure('demo.resource-not-found');
      const before = query.beforeCursor === undefined ? query.beforeSeq : Number(query.beforeCursor);
      const after = query.afterCursor === undefined ? query.afterSeq : Number(query.afterCursor);
      if ((before !== undefined && !Number.isSafeInteger(before)) || (after !== undefined && !Number.isSafeInteger(after)) || (before !== undefined && after !== undefined)) throw failure('demo.archive-cursor-invalid');
      const limit = Math.min(96, Math.max(1, query.pageSize ?? query.eventLimit ?? 96));
      if (!Number.isInteger(limit)) throw failure('demo.archive-cursor-invalid');
      const candidates = branch.pages.filter(page => (before === undefined || page.firstSeq < before) && (after === undefined || page.lastSeq > after));
      if (after === undefined) candidates.reverse();
      let events: AcpUiEventVm[] = [];
      for (const page of candidates) {
        const items = await read(page.path, eventsSchema, signal) as AcpUiEventVm[];
        const eligible = items.filter(item => (before === undefined || item.seq < before) && (after === undefined || item.seq > after));
        events = after === undefined ? [...eligible, ...events] : [...events, ...eligible];
        if (events.length >= limit) break;
      }
      events = after === undefined ? events.slice(-limit) : events.slice(0, limit);
      const first = events[0]?.seq;
      const last = events.at(-1)?.seq;
      return { snapshot: detail.snapshot, diagnostics: detail.diagnostics, workerRef: detail.workerRef,
        branches: detail.branches.map(branch => ({ id: branch.id, count: branch.count })), events, eventPage: {
        loadedCount: events.length, total: branch.count,
        oldestSeq: first ?? null, newestSeq: last ?? null,
        hasOlder: first !== undefined && first > (branch.pages[0]?.firstSeq ?? first),
        hasNewer: last !== undefined && last < (branch.pages.at(-1)?.lastSeq ?? last),
        oldestCursor: first === undefined ? null : String(first), newestCursor: last === undefined ? null : String(last),
      } };
    },
    async tool(session: ArchiveSession, branchId: string, eventId: string, signal?: AbortSignal) {
      const detail = await read(session.detail, detailSchema, signal);
      const path = detail.branches.find(branch => branch.id === branchId)?.tools[eventId];
      if (!path) throw failure('demo.resource-not-found');
      return read(path, eventsSchema.element, signal);
    },
    async activity(session: ArchiveSession, query: AcpActivityDetailQueryInput, signal?: AbortSignal) {
      const detail = await read(session.detail, detailSchema, signal);
      if (detail.snapshot?.sessionId !== query.sessionId) throw failure('demo.resource-not-found');
      const branch = detail.branches.find(branch => branch.id === query.branchId);
      if (!branch) throw failure('demo.resource-not-found');
      const cursor = query.earlierCursor == null ? query.activityEndSeq + 1 : Number(query.earlierCursor);
      const limit = Math.min(96, Math.max(1, query.limit ?? 48));
      if (![cursor, query.activityStartSeq, query.activityEndSeq, limit].every(Number.isSafeInteger)) throw failure('demo.archive-cursor-invalid');
      const items: AcpUiEventVm[] = [];
      const candidates = branch.pages.filter(page => page.firstSeq < cursor && page.lastSeq >= query.activityStartSeq).reverse();
      for (const page of candidates) {
        const events = await read(page.path, eventsSchema, signal) as AcpUiEventVm[];
        const eligible = events.filter(event => {
          const started = event.startedSeq ?? event.seq;
          const hidden = event.raw !== null && typeof event.raw === 'object' && 'hiddenFromChat' in event.raw && event.raw.hiddenFromChat === true;
          return started >= query.activityStartSeq && started <= query.activityEndSeq && started < cursor
            && event.sessionId === query.sessionId && !hidden
            && ['thoughtDelta', 'toolCall', 'toolCallUpdate', 'error'].includes(event.kind);
        });
        items.unshift(...eligible);
        if (items.length > limit) break;
      }
      const selected = items.slice(-limit);
      return { items: selected, hasMoreEarlier: items.length > limit,
        earlierCursor: selected.length ? String(selected[0].startedSeq ?? selected[0].seq) : null };
    },
    async rawPage(session: ArchiveSession, page = 0, pageSize = 100, order: 'asc' | 'desc' = 'desc', signal?: AbortSignal) {
      normalizeRawQuery({ page, pageSize, order });
      const { rawFrames: index } = await read(session.detail, detailSchema, signal);
      const start = order === 'asc' ? page * pageSize : Math.max(0, index.count - (page + 1) * pageSize);
      const end = order === 'asc' ? Math.min(index.count, start + pageSize) : index.count - page * pageSize;
      const selected = index.pages.filter(part => part.firstLine - 1 < end && part.firstLine - 1 + part.count > start);
      const items: AcpRawFrameVm[] = [];
      if (selected.length && end > start) {
        const offset = selected[0].offset;
        const last = selected.at(-1)!;
        const length = last.offset + last.length - offset;
        if (offset + length > index.bytes) throw failure('demo.archive-range-invalid');
        const response = await request(`${base}${index.resource}`, { headers: { Range: `bytes=${offset}-${offset + length - 1}` }, signal });
        if (response.status !== 206 || response.headers.get('Content-Range') !== `bytes ${offset}-${offset + length - 1}/${index.bytes}`) {
          await response.body?.cancel();
          throw failure('demo.archive-range-unsupported');
        }
        const buffer = await boundedBytes(response, length);
        const lines = new TextDecoder('utf-8', { fatal: true }).decode(buffer).trimEnd().split('\n');
        for (let i = 0; i < lines.length; i++) {
          const lineNumber = selected[0].firstLine + i;
          if (lineNumber <= start || lineNumber > end) continue;
          items.push(rawFrame(lines[i], lineNumber));
        }
      }
      if (order === 'desc') items.reverse();
      return { items, page, pageSize, total: index.count, hasPrevious: page > 0, hasNext: (page + 1) * pageSize < index.count, order };
    },
  };
}
export type ArchiveReader = ReturnType<typeof createArchiveReader>;
