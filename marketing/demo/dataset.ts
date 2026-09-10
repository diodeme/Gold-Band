import type { RuntimeApi } from '@/api/client';
import type { AcpRawFrameVm, AcpSessionVm, AcpUiEventVm, ConversationRunVm, ConversationSessionTargetVm, TurnFileChangeSetVm, FileComparisonVm, WorkspaceFileSnapshotVm } from '@/types';
import { cursorSequence, missing, pageLimit, pageSession, sessionIdentity } from './history-query';
import { includesDemoSession, selectDemoSessions } from './session-selection';

export const REAL_PROJECT_ID = 'e-projects-code-ai-ji--a40d4379';
type Resource = { path: string; byteLength: number; sha256: string };
type SessionReference = Resource & ConversationSessionTargetVm & { branchId: string; sessionId: string };
export interface DemoDataset {
  version: number;
  projectId: string;
  taskId: string;
  taskUuid: string;
  runId: string;
  title: string;
  source: { revision: number; status: string; outcome: string | null; pauseReason: string; startedAt?: string; updatedAt?: string };
  run: Resource;
  task: Resource;
  workflow: Resource;
  sessions: SessionReference[];
  resources?: Resource;
}
interface ResourceIndex {
  sessions: Record<string, Resource>;
}
interface SessionResources { changes: Record<string, Resource & { comparisonIndex: Resource }>; directory: Resource }
interface DirectoryEntry { name: string; relativePath: string; canonicalPath: string; kind: 'file' | 'directory'; hasChildren: boolean; byteLength: number | null; modifiedAtNs: string | null; resource: Resource }
interface EventReference extends Resource {
  id: string; seq: number; startedSeq: number; endedSeq: number; revision: number;
  kind: string; toolCallId?: string; branchId: string; sessionId: string; activityPage: number;
}
interface SessionIndex {
  session: AcpSessionVm;
  blocks: { itemIds: string[]; oldestSeq: number; newestSeq: number; lastRevision: number; summary?: AcpUiEventVm }[];
  eventRefs: Record<string, EventReference>;
  pages: Resource[];
  activityPages: Resource[];
  raw?: Resource;
}
export function createDatasetReader(base: string, fetcher: typeof fetch = fetch) {
  let datasetPromise: Promise<DemoDataset> | undefined;
  let currentIndex: { path: string; promise: Promise<SessionIndex> } | undefined;
  let resourcesPromise: Promise<ResourceIndex> | undefined;
  let currentResources: { path: string; promise: Promise<SessionResources> } | undefined;
  async function read<T>(resource: Resource | string): Promise<T> {
    const path = typeof resource === 'string' ? resource : resource.path;
    if (path.startsWith('/') || path.includes('..') || path.includes('\\') || path.includes(':')) missing({ path });
    const response = await fetcher(`${base}${path}`);
    if (!response.ok) missing({ path, status: response.status });
    const bytes = await response.arrayBuffer();
    if (typeof resource !== 'string') {
      const hash = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', bytes)), (byte) => byte.toString(16).padStart(2, '0')).join('');
      if (bytes.byteLength !== resource.byteLength || hash !== resource.sha256) throw { code: 'demo.resource-integrity', params: { path } };
    }
    return JSON.parse(new TextDecoder().decode(bytes)) as T;
  }
  function dataset() {
    datasetPromise ??= read<DemoDataset>('dataset.json').then((value) => {
      if (value.version !== 1 || value.projectId !== REAL_PROJECT_ID) throw { code: 'demo.resource-integrity', params: {} };
      return { ...value, sessions: value.sessions.filter(ref => includesDemoSession(value, ref)) };
    }).catch((error) => { datasetPromise = undefined; throw error; });
    return datasetPromise;
  }
  async function scope(projectId: string | null | undefined, taskId: string, runId: string) {
    const data = await dataset();
    if (projectId !== data.projectId || taskId !== data.taskId || runId !== data.runId) missing({ projectId, taskId, runId });
    return data;
  }
  async function resources() {
    const data = await dataset();
    if (!data.resources) missing({ resource: 'resources' });
    resourcesPromise ??= read<ResourceIndex>(data.resources).catch((error) => { resourcesPromise = undefined; throw error; });
    return resourcesPromise;
  }
  async function filesFor(locator: { projectId?: string | null; taskId: string; runId: string; roundId: string; nodeId: string; attemptId: string; outerNodeId?: string | null; outerAttemptId?: string | null }) {
    const data = await scope(locator.projectId, locator.taskId, locator.runId);
    const ref = data.sessions.find((ref) => ref.roundId === locator.roundId && ref.nodeId === locator.nodeId && ref.attemptId === locator.attemptId && ref.outerNodeId === locator.outerNodeId && ref.outerAttemptId === locator.outerAttemptId && ref.branchId === 'root');
    if (!ref) missing(locator);
    return sessionResources(ref.path.replace('/index.json', ''));
  }
  async function sessionResources(key: string) {
    const ref = (await resources()).sessions[key];
    if (!ref) missing({ key });
    if (currentResources?.path !== ref.path) currentResources = { path: ref.path, promise: read<SessionResources>(ref).catch((error) => { if (currentResources?.path === ref.path) currentResources = undefined; throw error; }) };
    return currentResources.promise;
  }
  async function directory(resource: Resource, path: string) {
    if (path.includes('..') || path.includes('\\') || path.startsWith('/')) missing({ path });
    let entries = await read<DirectoryEntry[]>(resource);
    for (const name of path.split('/').filter(Boolean)) {
      const child = entries.find((entry) => entry.kind === 'directory' && entry.name === name);
      if (!child) missing({ path });
      entries = await read<DirectoryEntry[]>(child.resource);
    }
    return entries;
  }
  async function attachment(resource: Resource, path: string) {
    const segments = path.split('/'); const name = segments.pop();
    const entry = (await directory(resource, segments.join('/'))).find((entry) => entry.name === name && entry.kind === 'file');
    if (!entry) missing({ path });
    return entry;
  }
  async function session(projectId: string | null | undefined, taskId: string, runId: string, roundId: string, nodeId: string, attemptId: string, outerNodeId?: string | null, outerAttemptId?: string | null, branchId = 'root') {
    const data = await scope(projectId, taskId, runId);
    const ref = data.sessions.find((ref) => ref.roundId === roundId && ref.nodeId === nodeId && ref.attemptId === attemptId && ref.outerNodeId === outerNodeId && ref.outerAttemptId === outerAttemptId && ref.branchId === branchId);
    if (!ref) missing({ roundId, nodeId, attemptId, outerNodeId, outerAttemptId, branchId });
    if (currentIndex?.path !== ref.path) currentIndex = { path: ref.path, promise: read<SessionIndex>(ref).catch((error) => { if (currentIndex?.path === ref.path) currentIndex = undefined; throw error; }) };
    return currentIndex.promise;
  }
  const api: Pick<RuntimeApi, 'getConversationRun' | 'getAcpSession' | 'getAcpActivityDetail' | 'getAcpToolDetail' | 'getAcpRawFrames' | 'getTurnFileChangeSet' | 'getFileComparison' | 'resolveTurnAttachmentFile' | 'readFileResource' | 'listConversationDirectory' | 'readConversationDirectoryFile' | 'workspaceFilePreviewUrl'> = {
    workspaceFilePreviewUrl(token) {
      if (!/^demo-history\/sessions\/[a-f0-9]{24}\/images\/[a-f0-9]{64}\.(png|jpg|gif|webp|bmp|ico|avif)$/.test(token)) missing({ token });
      return `${base}${token.slice('demo-history/'.length)}`;
    },
    async getAcpRawFrames(projectId, taskId, runId, roundId, nodeId, attemptId, query = {}, outerNodeId, outerAttemptId) {
      const index = await session(projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId);
      if (!index.raw) missing({ resource: 'raw' });
      const page = query.page ?? 0;
      if (!Number.isSafeInteger(page) || page < 0 || (query.pageSize != null && (!Number.isSafeInteger(query.pageSize) || query.pageSize < 1))) throw { code: 'demo.invalid-query', params: {} };
      const pageSize = Math.max(25, Math.min(200, query.pageSize ?? 100));
      const order = query.order ?? 'desc';
      if (order !== 'asc' && order !== 'desc') throw { code: 'demo.invalid-query', params: {} };
      const search = query.search?.trim().toLowerCase() || null;
      const kind = query.kind?.trim().toLowerCase() || null;
      const direction = query.direction?.trim().toLowerCase() || null;
      const raw = await read<{ frames: (Omit<AcpRawFrameVm, 'content'> & { page: number })[]; pages: Resource[] }>(index.raw);
      let refs = raw.frames.filter((frame) => (!kind || frame.kind.toLowerCase().includes(kind)) && (!direction || frame.direction?.toLowerCase() === direction));
      if (search) {
        const matching = new Set<string>();
        for (const id of new Set(refs.map((frame) => frame.page))) {
          for (const frame of await read<AcpRawFrameVm[]>(raw.pages[id])) if (frame.content.toLowerCase().includes(search)) matching.add(frame.id);
        }
        refs = refs.filter((frame) => matching.has(frame.id));
      }
      if (order === 'desc') refs.reverse();
      const selected = refs.slice(page * pageSize, (page + 1) * pageSize);
      const pages = await Promise.all([...new Set(selected.map((frame) => frame.page))].map((id) => read<AcpRawFrameVm[]>(raw.pages[id])));
      const byId = new Map(pages.flat().map((frame) => [frame.id, frame]));
      return { items: selected.map((frame) => byId.get(frame.id)!), page, pageSize, total: refs.length, hasPrevious: page > 0 && refs.length > 0, hasNext: (page + 1) * pageSize < refs.length, order, search, kind, direction };
    },
    async getTurnFileChangeSet(locator, id) {
      const ref = (await filesFor(locator)).changes[id];
      if (!ref) missing({ id });
      const value = await read<TurnFileChangeSetVm>(ref);
      if (value.branchId !== locator.branchId) missing({ branchId: locator.branchId });
      return value;
    },
    async getFileComparison(locator, changeSetId, changeId) {
      await api.getTurnFileChangeSet(locator, changeSetId);
      const indexRef = (await filesFor(locator)).changes[changeSetId]?.comparisonIndex;
      if (!indexRef) missing({ changeSetId });
      const ref = (await read<Record<string, Resource>>(indexRef))[changeId];
      if (!ref) missing({ changeId });
      return read<FileComparisonVm>(ref);
    },
    async resolveTurnAttachmentFile(locator, changeSetId, attachmentId) {
      const changes = await api.getTurnFileChangeSet(locator, changeSetId);
      const attachment = changes.attachments.find((item) => item.id === attachmentId);
      if (!attachment) missing({ attachmentId });
      const entry = await findAttachment(locator, `attachments/${attachment.relativePath}`);
      return { locator: { projectId: locator.projectId, canonicalPath: entry.canonicalPath, relativePath: attachment.relativePath, scope: 'workspace' }, target: null, externalAccessGrant: null };
    },
    async readFileResource(projectId, path) {
      if (projectId !== (await dataset()).projectId) missing({ projectId });
      const match = /^\/demo-history\/(sessions\/[a-f0-9]{24})\/files\/(.+)$/.exec(path);
      if (!match) missing({ path });
      const entry = await attachment((await sessionResources(match[1])).directory, match[2]);
      return read<WorkspaceFileSnapshotVm>(entry.resource);
    },
    async listConversationDirectory(input) {
      const entries = await directory((await filesFor(input)).directory, input.relativePath ?? '');
      return entries.map(({ resource: _resource, ...entry }) => entry);
    },
    async readConversationDirectoryFile(input) {
      const entry = await findAttachment(input, input.relativePath ?? '');
      return read<WorkspaceFileSnapshotVm>(entry.resource);
    },
    async getConversationRun(projectId, taskId, runId) {
      return selectDemoSessions(await read<ConversationRunVm>((await scope(projectId, taskId, runId)).run));
    },
    async getAcpSession(projectId, taskId, runId, roundId, nodeId, attemptId, query, _fallback, outerNodeId, outerAttemptId) {
      const index = await session(projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId, query?.branchId);
      const logical = index.blocks.map((block, position) => ({ id: String(position), seq: block.oldestSeq, startedSeq: block.oldestSeq, endedSeq: block.newestSeq, kind: 'index', timestamp: '', timing: { revision: block.lastRevision, sessionElapsedSeconds: 0, paused: true } }));
      const projection = pageSession({ ...index.session, events: logical }, query);
      const selected = new Set(projection.events.map((event) => Number(event.id)));
      const pageIds = [...new Set([...selected].map((position) => Math.floor(position / 50)))];
      const pages = await Promise.all(pageIds.map((id) => read<AcpUiEventVm[]>(index.pages[id])));
      const ids = new Set([...selected].flatMap((position) => index.blocks[position].summary ? [index.blocks[position].summary!.id] : index.blocks[position].itemIds));
      const events = pages.flat().filter((event) => ids.has(event.id));
      return { ...projection, events, eventPage: { ...projection.eventPage, loadedCount: events.length } };
    },
    async getAcpActivityDetail(projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId) {
      const index = await session(projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId, query.branchId);
      sessionIdentity(index.session, query);
      const refs = Object.values(index.eventRefs).filter((ref) => ref.startedSeq >= query.activityStartSeq && ref.endedSeq <= query.activityEndSeq && ref.branchId === query.branchId).sort((a, b) => a.startedSeq - b.startedSeq || a.seq - b.seq);
      const before = cursorSequence(query.earlierCursor);
      const end = before == null ? refs.length : refs.findIndex((ref) => ref.startedSeq >= before);
      if (end < 0) throw { code: 'demo.invalid-cursor', params: {} };
      const selected = refs.slice(Math.max(0, end - pageLimit(query.limit)), end);
      const ids = new Set(selected.map((ref) => ref.id));
      const pages = await Promise.all([...new Set(selected.map((ref) => ref.activityPage))].map((id) => read<AcpUiEventVm[]>(index.activityPages[id])));
      const items = pages.flat().filter((event) => ids.has(event.id));
      return { items, hasMoreEarlier: end > selected.length, earlierCursor: end > selected.length ? `rev:${selected[0].startedSeq}` : null };
    },
    async getAcpToolDetail(projectId, taskId, runId, roundId, nodeId, attemptId, query, outerNodeId, outerAttemptId) {
      const index = await session(projectId, taskId, runId, roundId, nodeId, attemptId, outerNodeId, outerAttemptId, query.branchId);
      sessionIdentity(index.session, query);
      const ref = index.eventRefs[query.eventId];
      if (!ref || ref.kind !== 'toolCall' || ref.branchId !== query.branchId || (query.toolCallId && ref.toolCallId !== query.toolCallId)) missing({ eventId: query.eventId });
      return { event: await read<AcpUiEventVm>(ref) };
    },
  };
  async function findAttachment(locator: Parameters<typeof filesFor>[0], path: string) {
    return attachment((await filesFor(locator)).directory, path);
  }
  return { dataset, api };
}
