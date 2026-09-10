import { describe, expect, it } from 'vitest';
import { createDemoApi } from '../../marketing/demo/api';
import { createArchiveReader } from '../../marketing/demo/archive';
import { withArchiveSidebar } from '../../marketing/demo/archive-run';
import { demoSidebar } from '../../marketing/demo/fixtures';
import { selectDemoSession } from '../../marketing/demo/routes';
import { projectArchiveRun } from '../../scripts/project-demo-run.mjs';
import { createHash } from 'node:crypto';

const asset = (n: number) => `assets/${String(n).padStart(64, '0')}.json`;
function fixture() {
  const before = 'Before 中文\n';
  const after = 'After 中文\n';
  const resource = (content: string) => {
    const sha256 = createHash('sha256').update(content).digest('hex');
    return { path: `resources/${sha256}`, bytes: new TextEncoder().encode(content).length, sha256 };
  };
  const beforeResource = resource(before);
  const afterResource = resource(after);
  const version = (entry: ReturnType<typeof resource>) => ({ id: entry.sha256, storageKind: 'capturedBlob', contentHash: entry.sha256, byteLength: entry.bytes });
  const changeSet = { id: 'set', turnId: 'turn', promptEventId: 'prompt', branchId: 'root', status: 'finalized', startedAt: '1Z', finishedAt: '2Z',
    summary: { fileCount: 1, addedFiles: 0, modifiedFiles: 1, deletedFiles: 0, addedLines: 1, deletedLines: 1 },
    changes: [{ id: 'change', changeKind: 'modified', logicalPath: 'docs/report.md', text: true, beforeVersion: version(beforeResource), afterVersion: version(afterResource) }],
    attachments: [{ id: 'attachment', relativePath: 'report.md', name: 'report.md', byteLength: afterResource.bytes }], limitationCodes: [] };
  const session = { projectId: 'source', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt',
    node: { id: 'node', title: 'Original node', status: 'paused', outcome: null, startedAt: '1Z' }, detail: asset(2), itemCount: 3, recordCount: 3 };
  const catalog = { version: 1, runtimeVersion: 1, projectId: 'source', task: { id: 'task', title: 'Original task' },
    run: { id: 'run', status: 'paused', outcome: null, startedAt: '1Z', updatedAt: '2Z' }, workflow: asset(4), runView: asset(1), sessions: [session] };
  const run = projectArchiveRun({ ...catalog, run: { ...catalog.run, current_round: 'round', current_node: 'node', current_attempt: 'attempt' } },
    [{ id: 'round', index: 1, status: 'paused', outcome: null }], []);
  const events = [
    { id: 'text', seq: 1, kind: 'textDelta', content: 'Original text', timestamp: '1Z', sessionId: 'connection' },
    { id: 'thought', seq: 2, kind: 'thoughtDelta', content: 'Original thought', timestamp: '1Z', sessionId: 'connection' },
    { id: 'tool', seq: 3, kind: 'toolCall', toolCallId: 'call', timestamp: '2Z', sessionId: 'connection' },
  ];
  const resources: Record<string, unknown> = { 'catalog.json': catalog, [asset(1)]: run, [asset(3)]: events,
    [asset(7)]: { entries: [{ name: 'attachments', kind: 'directory', directory: asset(8), hasChildren: true },
      { name: 'acp.raw.jsonl', kind: 'file', resource: beforeResource, image: null }] },
    [asset(8)]: { entries: [{ name: 'report.md', kind: 'file', resource: afterResource, image: null }] },
    [beforeResource.path]: before, [afterResource.path]: after,
    [asset(6)]: { changeSet, versions: { [beforeResource.sha256]: beforeResource, [afterResource.sha256]: afterResource },
      attachments: { attachment: { ...afterResource, kind: 'text', encoding: 'utf-8', lineEnding: 'lf' } } },
    [asset(5)]: { ...events[2], raw: { output: 'Original full output' } },
    [asset(2)]: { directoryRoot: asset(7), changeSets: { set: asset(6) }, snapshot: { sessionId: 'connection', latestTurnStatus: 'paused' }, workerRef: { provider: 'codex-acp' },
      diagnostics: { rawFrameCount: 0, eventCount: 3, errorCount: 0, lastError: null, lastErrorTimestamp: null },
      rawFrames: { resource: `resources/${'0'.repeat(64)}`, bytes: 0, count: 0, pages: [] },
      branches: [{ id: 'root', count: 3, tools: { tool: asset(5) }, pages: [{ path: asset(3), firstSeq: 1, lastSeq: 3, count: 3 }] }] } };
  const calls: string[] = [];
  const reader = createArchiveReader('/archive', async input => {
    const path = String(input).replace('/archive/', '');
    calls.push(path);
    return path.startsWith('resources/') ? new Response(resources[path] as string) : Response.json(resources[path]);
  });
  const api = createDemoApi(undefined, { reader, catalog: () => reader.catalog() });
  const args = ['source', 'task', 'run', 'round', 'node', 'attempt'] as const;
  return { api, reader, args, calls, changeSet, resources, afterResource };
}
describe('archive integration with the readonly client API', () => {
  it('opens the source version associated with the exact message and preserves its line target', async () => {
    const { api, resources, afterResource, calls } = fixture();
    const locator = { projectId: 'source', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt', branchId: 'root' };
    const href = '/E:/source/main.rs:2:3';
    const beforeHash = createHash('sha256').update('Before 中文\n').digest('hex');
    const beforeResource = { path: `resources/${beforeHash}`, sha256: beforeHash, bytes: Buffer.byteLength('Before 中文\n') };
    const index = { version: 1, locator, references: [
      { branchId: 'root', eventId: 'text', href, sourcePath: 'E:/source/main.rs', resource: afterResource },
      { branchId: 'root', eventId: 'thought', href, sourcePath: 'E:/source/main.rs', resource: beforeResource },
    ], unresolved: [] };
    const content = JSON.stringify(index);
    const path = `assets/${createHash('sha256').update(content).digest('hex')}.json`;
    resources[path] = index;
    (resources[asset(2)] as Record<string, unknown>).sourceReferences = path;
    const resolved = await api.resolveWorkspaceFileLink('source', href, null, { locator, eventId: 'text' });
    expect(resolved).toMatchObject({ locator: { relativePath: 'E:/source/main.rs' }, target: { line: 2, column: 3 } });
    expect(calls).not.toContain(afterResource.path);
    expect(await api.readFileResource('source', resolved.locator.canonicalPath)).toMatchObject({ kind: 'text', name: 'main.rs', content: 'After 中文\n', editable: false });
    const earlier = await api.resolveWorkspaceFileLink('source', href, null, { locator, eventId: 'thought' });
    expect(earlier.locator.canonicalPath).not.toBe(resolved.locator.canonicalPath);
    expect(await api.readFileResource('source', earlier.locator.canonicalPath)).toMatchObject({ content: 'Before 中文\n', editable: false });
    expect((await api.resolveWorkspaceFileLink('source', resolved.locator.canonicalPath)).locator).toEqual(resolved.locator);
    await expect(api.resolveWorkspaceFileLink('source', href, null, { locator, eventId: 'other' })).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(api.resolveWorkspaceFileLink('source', href, null, { locator: { ...locator, outerNodeId: 'other' }, eventId: 'text' })).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(api.readFileResource('other', resolved.locator.canonicalPath)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(api.readFileResource('source', resolved.locator.canonicalPath.replace('/main.rs', '/other.rs'))).rejects.toMatchObject({ code: 'demo.resource-not-found' });
  });
  it('opens a captured absolute source reference after verifying the original attempt directory', async () => {
    const { api, resources, calls, afterResource } = fixture();
    const root = 'C:/Users/source/.gold-band/projects/source/tasks/task/runs/run/rounds/round/nodes/node/attempt';
    const content = JSON.stringify({ continue_ref: { snapshotFile: `${root}/acp.snapshot.json` } });
    const sha256 = createHash('sha256').update(content).digest('hex');
    const worker = { path: `resources/${sha256}`, sha256, bytes: Buffer.byteLength(content) };
    resources[worker.path] = content;
    (resources[asset(7)] as { entries: unknown[] }).entries.push({ name: 'worker-ref.json', kind: 'file', resource: worker, image: null });
    const resolved = await api.resolveWorkspaceFileLink('source', `${root}/attachments/report.md#L2`);
    expect(resolved).toMatchObject({ locator: { relativePath: 'attachments/report.md' }, target: { line: 2 }, externalAccessGrant: null });
    expect(calls.filter(path => path.startsWith('resources/'))).toEqual([worker.path]);
    expect(calls).not.toContain(afterResource.path);
    expect((await api.resolveWorkspaceFileLink('source', `${root}/attachments/report.md#L2`, null, {
      locator: { projectId: 'source', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt', branchId: 'root' }, eventId: 'text',
    })).locator).toEqual(resolved.locator);
    expect(await api.readFileResource('source', resolved.locator.canonicalPath)).toMatchObject({ kind: 'text', content: 'After 中文\n', editable: false });
    for (const href of [`${root.replace('C:', 'D:')}/attachments/report.md`, `${root}/../attempt/attachments/report.md`, `${root}/attachments/missing.md`]) {
      await expect(api.resolveWorkspaceFileLink('source', href)).rejects.toHaveProperty('code');
    }
    await expect(api.resolveWorkspaceFileLink('other', `${root}/attachments/report.md`)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    expect((await api.resolveWorkspaceFileLink('source', `file:///${root.toLowerCase()}/attachments/REPORT.md:3:2`)).target).toEqual({ line: 3, column: 2, endLine: null });
  });
  it('resolves the nested attempt without probing its parent and rejects unverified source roots', async () => {
    const { api, resources, calls } = fixture();
    const catalog = resources['catalog.json'] as { sessions: Record<string, unknown>[] };
    const child = catalog.sessions[0];
    Object.assign(child, { outerNodeId: 'outer', outerAttemptId: 'outer-attempt' });
    catalog.sessions.push({ ...child, nodeId: 'outer', attemptId: 'outer-attempt', outerNodeId: undefined, outerAttemptId: undefined, detail: asset(99) });
    const root = 'C:/Users/source/.gold-band/projects/source/tasks/task/runs/run/rounds/round/nodes/outer/outer-attempt/dynamic/nodes/node/attempt';
    const content = JSON.stringify({ continue_ref: { snapshotFile: `${root}/acp.snapshot.json` } });
    const sha256 = createHash('sha256').update(content).digest('hex');
    const worker = { path: `resources/${sha256}`, sha256, bytes: Buffer.byteLength(content) };
    resources[worker.path] = content;
    (resources[asset(7)] as { entries: unknown[] }).entries.push({ name: 'worker-ref.json', kind: 'file', resource: worker, image: null });
    const resolved = await api.resolveWorkspaceFileLink('source', `${root}/attachments/report.md`);
    expect(decodeURIComponent(resolved.locator.canonicalPath)).toContain('"outer","outer-attempt"');
    expect(calls).not.toContain(asset(99));
    for (const href of [`file:///${root}/%2e%2e/attempt/attachments/report.md`, `${root.replace('Users/source', 'Users/other')}/attachments/report.md`, `file://server/${root}/attachments/report.md`]) {
      await expect(api.resolveWorkspaceFileLink('source', href)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    }
    worker.bytes = 64 * 1024 + 1;
    calls.length = 0;
    await expect(api.resolveWorkspaceFileLink('source', `${root}/attachments/report.md`)).rejects.toMatchObject({ code: 'workspace-file.too-large' });
    expect(calls.some(path => path.startsWith('resources/'))).toBe(false);
    for (const invalid of ['not json', '{}']) {
      const invalidHash = createHash('sha256').update(invalid).digest('hex');
      Object.assign(worker, { path: `resources/${invalidHash}`, sha256: invalidHash, bytes: Buffer.byteLength(invalid) });
      resources[worker.path] = invalid;
      await expect(api.resolveWorkspaceFileLink('source', `${root}/attachments/report.md`)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    }
  });
  it('resolves relative directory links from the source document without synthesizing files', async () => {
    const { api, calls } = fixture();
    const [document] = await api.listConversationDirectory({ projectId: 'source', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt', relativePath: 'attachments' });
    const linked = await api.resolveWorkspaceFileLink('source', '../acp.raw.jsonl#L2-L3', document.canonicalPath);
    expect(linked).toMatchObject({ locator: { relativePath: 'acp.raw.jsonl', scope: 'workspace' }, target: { line: 2, endLine: 3, column: null }, externalAccessGrant: null });
    expect(calls.some(path => path.startsWith('resources/'))).toBe(false);
    await expect(api.resolveWorkspaceFileLink('source', 'missing.md', document.canonicalPath)).rejects.toMatchObject({ code: 'conversation-directory.not-found' });
    await expect(api.resolveWorkspaceFileLink('source', '../../outside.md', document.canonicalPath)).rejects.toMatchObject({ code: 'conversation-directory.path-outside-root' });
    await expect(api.resolveWorkspaceFileLink('other', document.canonicalPath)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(api.resolveWorkspaceFileLink('source', '%2e%2e/%2e%2e/outside.md', document.canonicalPath)).rejects.toMatchObject({ code: 'conversation-directory.path-outside-root' });
    await expect(api.resolveWorkspaceFileLink('source', 'C:\\private\\notes.md', document.canonicalPath)).rejects.toMatchObject({ code: 'conversation-directory.path-outside-root' });
    await expect(api.resolveWorkspaceFileLink('source', '%invalid', document.canonicalPath)).rejects.toMatchObject({ code: 'demo.archive-path-invalid' });
    expect((await api.resolveWorkspaceFileLink('source', './REPORT.md:4:2', document.canonicalPath)).target).toEqual({ line: 4, column: 2, endLine: null });
  });
  it('resolves a local Markdown image using archived metadata and a static resource grant', async () => {
    const { api, calls, resources, afterResource } = fixture();
    (resources[asset(8)] as { entries: unknown[] }).entries.push({ name: 'diagram.png', kind: 'file', resource: afterResource,
      image: { mimeType: 'image/png', width: 820, height: 900, animated: false } });
    const [document] = await api.listConversationDirectory({ projectId: 'source', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt', relativePath: 'attachments' });
    const input = { projectId: 'source', markdownCanonicalPath: document.canonicalPath, markdownExternalAccessToken: null, rawSrc: './DIAGRAM.png', approvedExternalTargets: [] };
    const image = await api.resolveMarkdownImage(input);
    expect(image).toMatchObject({ kind: 'ready', width: 820, height: 900, previewGrant: { token: `archive-resource:${afterResource.sha256}` } });
    expect(calls.some(path => path.startsWith('resources/'))).toBe(false);
    await expect(api.resolveMarkdownImage({ ...input, rawSrc: 'missing.png' })).rejects.toMatchObject({ code: 'conversation-directory.not-found' });
    await expect(api.resolveMarkdownImage({ ...input, projectId: 'other' })).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(api.resolveMarkdownImage({ ...input, rawSrc: 'https://example.com/image.png' })).rejects.toMatchObject({ code: 'workspace-file.markdown-image-network-blocked' });
    expect(await api.resolveMarkdownImage({ ...input, rawSrc: 'report.md' })).toMatchObject({ kind: 'unsupported' });
    (resources[asset(8)] as { entries: { image?: { width: number } }[] }).entries[1].image!.width = 50_000_000;
    expect(await api.resolveMarkdownImage(input)).toMatchObject({ kind: 'unsupported', limitationCode: 'workspace-file.too-large' });
  });
  it('resolves attachment-document links against their original directory', async () => {
    const { api } = fixture();
    const link = await api.resolveTurnAttachmentFile({ projectId: 'source', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt', branchId: 'root' }, 'set', 'attachment');
    const resolved = await api.resolveWorkspaceFileLink('source', 'report.md', link.locator.canonicalPath);
    expect(resolved.locator.relativePath).toBe('attachments/report.md');
    expect((await api.resolveWorkspaceFileLink('source', link.locator.canonicalPath)).locator).toEqual(resolved.locator);
    await expect(api.resolveWorkspaceFileLink('default', 'report.md', link.locator.canonicalPath)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(api.resolveMarkdownImage({ projectId: 'default', markdownCanonicalPath: link.locator.canonicalPath, rawSrc: 'image.png', markdownExternalAccessToken: null, approvedExternalTargets: [] })).rejects.toMatchObject({ code: 'demo.resource-not-found' });
  });
  it('lists the source attempt directory instead of a sample reports folder', async () => {
    const { api, calls } = fixture();
    const entries = await api.listConversationDirectory({ projectId: 'source', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt' });
    expect(entries.map(entry => entry.name)).toEqual(['attachments', 'acp.raw.jsonl']);
    expect(calls.some(path => path.startsWith('resources/'))).toBe(false);
  });
  it('reads canonical source directory files and rejects path traversal and scope substitution', async () => {
    const { api, calls, resources } = fixture();
    const locator = { projectId: 'source', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt', outerNodeId: 'outer', outerAttemptId: 'outer-attempt' };
    Object.assign((resources['catalog.json'] as { sessions: object[] }).sessions[0], { outerNodeId: locator.outerNodeId, outerAttemptId: locator.outerAttemptId });
    const entries = await api.listConversationDirectory({ ...locator, relativePath: 'ATTACHMENTS' });
    expect(entries[0].relativePath).toBe('attachments/report.md');
    expect(entries[0].canonicalPath.endsWith('/attachments/report.md')).toBe(true);
    expect(calls.some(path => path.startsWith('resources/'))).toBe(false);
    const snapshot = await api.readConversationDirectoryFile({ ...locator, relativePath: 'ATTACHMENTS\\REPORT.md' });
    expect(snapshot).toMatchObject({ kind: 'text', content: 'After 中文\n', editable: false, language: 'markdown', locator: { canonicalPath: entries[0].canonicalPath } });
    await expect(api.readFileResource('other', entries[0].canonicalPath)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(api.listConversationDirectory({ ...locator, outerAttemptId: 'other' })).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(api.listConversationDirectory({ ...locator, relativePath: '../other' })).rejects.toMatchObject({ code: 'conversation-directory.path-outside-root' });
    await expect(api.readConversationDirectoryFile({ ...locator, relativePath: 'attachments/missing.md' })).rejects.toMatchObject({ code: 'conversation-directory.not-found' });
  });
  it('accepts raw filters for an empty source log', async () => {
    const { api, args } = fixture();
    expect(await api.getAcpRawFrames(...args, { search: ' Missing ', kind: ' TOOL ', direction: ' IN ' }))
      .toMatchObject({ items: [], total: 0, search: 'missing', kind: 'tool', direction: 'in' });
  });
  it('preserves the shared raw panel page size independently of storage chunks', async () => {
    const { api, args } = fixture();
    expect(await api.getAcpRawFrames(...args, { pageSize: 100 })).toMatchObject({ pageSize: 100, total: 0 });
  });
  it('gives image previews a renewable future expiry without loading the body', async () => {
    const { api, resources, afterResource, calls } = fixture();
    const files = resources[asset(6)] as { attachments: Record<string, unknown> };
    files.attachments.attachment = { ...afterResource, kind: 'image', mimeType: 'image/png', width: 10, height: 10, animated: false };
    const locator = { projectId: 'source', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt', branchId: 'root' };
    const link = await api.resolveTurnAttachmentFile(locator, 'set', 'attachment');
    const startedAt = Date.now();
    const snapshot = await api.readFileResource('source', link.locator.canonicalPath);
    expect(snapshot.kind).toBe('image');
    if (snapshot.kind !== 'image') throw new Error('Expected image');
    expect(Number(snapshot.previewGrant.expiresAtMs)).toBeGreaterThan(startedAt + 60_000);
    expect(Number(snapshot.previewGrant.expiresAtMs) - startedAt).toBeLessThan(2 ** 31 - 1);
    expect(api.workspaceFilePreviewUrl(snapshot.previewGrant.token)).toContain(afterResource.path);
    expect(calls.some(path => path.startsWith('resources/'))).toBe(false);
  });
  it('loads file lists separately from immutable Diff and attachment bodies', async () => {
    const { api, calls } = fixture();
    const locator = { projectId: 'source', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt', branchId: 'root' };
    const files = await api.getTurnFileChangeSet(locator, 'set');
    expect(files.attachments[0].name).toBe('report.md');
    expect(calls.some(path => path.startsWith('resources/'))).toBe(false);
    const comparison = await api.getFileComparison(locator, 'set', 'change');
    expect(comparison.before?.content).toBe('Before 中文\n');
    expect(comparison.after?.content).toBe('After 中文\n');
    const link = await api.resolveTurnAttachmentFile(locator, 'set', 'attachment');
    const snapshot = await api.readFileResource('source', link.locator.canonicalPath);
    expect(snapshot).toMatchObject({ kind: 'text', content: 'After 中文\n', editable: false, language: 'markdown' });
    await expect(api.getTurnFileChangeSet({ ...locator, branchId: 'other' }, 'set')).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(api.readFileResource('other-project', link.locator.canonicalPath)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(api.resolveTurnAttachmentFile(locator, 'set', 'unknown')).rejects.toMatchObject({ code: 'demo.resource-not-found' });
  });
  it('rejects corrupted attachment bytes and avoids fetching an oversized comparison', async () => {
    const { api, changeSet, calls, resources, afterResource } = fixture();
    const locator = { projectId: 'source', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt', branchId: 'root' };
    changeSet.changes[0].beforeVersion.byteLength = 3 * 1024 * 1024;
    expect((await api.getFileComparison(locator, 'set', 'change')).limitationCode).toBe('turn-files.diff-too-large');
    expect(calls.some(path => path.startsWith('resources/'))).toBe(false);
    resources[afterResource.path] = 'Other 中文\n';
    const link = await api.resolveTurnAttachmentFile(locator, 'set', 'attachment');
    await expect(api.readFileResource('source', link.locator.canonicalPath)).rejects.toMatchObject({ code: 'demo.archive-resource-corrupt' });
  });
  it('loads run identity before history and selects its exact source locator', async () => {
    const { api, args, calls } = fixture();
    const run = await api.getConversationRun(...args.slice(0, 3) as [string, string, string]);
    expect(run.runStatus).toBe('paused');
    expect(run.runOutcome).toBeNull();
    expect(calls).toEqual(['catalog.json', asset(1)]);
    expect(selectDemoSession(run, new URLSearchParams()).attemptId).toBe('attempt');
    const session = await api.getAcpSession(...args, { pageSize: 2 });
    expect(session?.events.map(event => event.id)).toEqual(['thought', 'tool']);
    expect(session?.readOnly).toBe(true);
    expect(calls).not.toContain(asset(5));
  });
  it('isolates activity, tool and raw access by source locator and ACP connection', async () => {
    const { api, args } = fixture();
    const activity = { branchId: 'root', sessionId: 'connection', activityStartSeq: 1, activityEndSeq: 3, limit: 1 };
    const page = await api.getAcpActivityDetail(...args, activity);
    expect(page.items.map(event => event.id)).toEqual(['tool']);
    expect(page.hasMoreEarlier).toBe(true);
    expect((await api.getAcpActivityDetail(...args, { ...activity, earlierCursor: page.earlierCursor })).items.map(event => event.id)).toEqual(['thought']);
    expect((await api.getAcpToolDetail(...args, { branchId: 'root', sessionId: 'connection', eventId: 'tool', toolCallId: 'call' })).event?.raw).toEqual({ output: 'Original full output' });
    await expect(api.getAcpToolDetail(...args, { branchId: 'root', sessionId: 'wrong', eventId: 'tool' })).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(api.getAcpSession(...args, {}, null, 'wrong', 'attempt')).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    expect((await api.getAcpRawFrames(...args)).total).toBe(0);
  });
  it('adds one original task and run without replacing existing samples', async () => {
    const { reader, api } = fixture();
    const sidebar = withArchiveSidebar(demoSidebar('en'), await reader.catalog());
    expect(sidebar.tasksByWorkspace.default).toHaveLength(2);
    expect(sidebar.tasksByWorkspace.source[0].title).toBe('Original task');
    expect(sidebar.tasksByWorkspace.source[0].runs).toHaveLength(1);
    await expect(api.getConversationRun('source', 'task', 'other-run')).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    await expect(api.stopActiveSession('source', 'task', 'run', 'round', 'node', 'attempt')).rejects.toMatchObject({ code: 'demo.operation-unavailable' });
  });
});
