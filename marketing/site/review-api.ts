import { z } from 'zod';
import type { RuntimeApi } from '@/api/client';
import type { FileRevisionVm, GitFileChangeVm, GitSourceControlSnapshotVm, TurnFileLocatorVm, WorkspaceFileChangedEventVm } from '@/types';
import type { Language } from './content';
import fixture from './review-content.json';

const root = '/default';
const changeSetId = 'browser-change-set-052';
const fileSchema = z.object({ path: z.string(), content: z.string(), hash: z.string(), bytes: z.number(), modifiedAtNs: z.string() });
const statusSchema = z.object({ root: z.string(), initialHead: z.string(), head: z.string(), gitVersion: z.string(), revision: z.string(),
  changes: z.array(z.object({ index: z.string(), worktree: z.string(), path: z.string() })) });
const comparisonSchema = z.object({ path: z.string(), before: z.string().nullable(), after: z.string().nullable(), stats: z.object({ addedLines: z.number(), deletedLines: z.number() }) });
const revision = (file: z.infer<typeof fileSchema>): FileRevisionVm => ({ contentHash: file.hash, byteLength: file.bytes, modifiedAtNs: file.modifiedAtNs });
const fail = () => { throw { code: 'site.review-operation-rejected', params: {} }; };
export async function createReviewApi(original: RuntimeApi, language: Language, locator: TurnFileLocatorVm) {
  const send = async (body: unknown) => {
    const response = await fetch('/__site-review', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body), keepalive: true });
    const result: unknown = await response.json();
    if (!response.ok) throw result;
    return result;
  };
  const { session } = z.object({ session: z.string() }).parse(await send({ action: 'initialize', language }));
  const request = (command: unknown) => send({ action: 'execute', session, command });
  let closed = false;
  const dispose = async () => { if (!closed) { closed = true; await send({ action: 'close', session }); } };
  try {
    const captures = await Promise.all(fixture.files.map(async file => comparisonSchema.parse(await request({ action: 'comparison', path: file.path, area: 'unstaged' }))));
    const changes = await original.getTurnFileChangeSet(locator, changeSetId);
    changes.changes = changes.changes.filter(change => captures.some(file => file.path === change.logicalPath))
      .map(change => ({ ...change, ...captures.find(file => file.path === change.logicalPath)!.stats }));
    changes.summary = { fileCount: 2, addedFiles: 1, modifiedFiles: 1, deletedFiles: 0,
      addedLines: captures.reduce((sum, file) => sum + file.stats.addedLines, 0), deletedLines: captures.reduce((sum, file) => sum + file.stats.deletedLines, 0) };
    changes.attachments = changes.attachments.filter(attachment => attachment.name === 'report.md');
    const attachment = changes.attachments[0];
    attachment.byteLength = new TextEncoder().encode(fixture.report[language]).length;
    const report = await original.resolveTurnAttachmentFile(locator, changeSetId, attachment.id);
    const snapshot = await original.readFileResource(locator.projectId, report.locator.canonicalPath, report.externalAccessGrant?.token);
    if (snapshot.kind !== 'text') fail();
    await original.writeFileResource({ projectId: locator.projectId, canonicalPath: report.locator.canonicalPath, externalAccessToken: report.externalAccessGrant?.token ?? null,
      expectedRevision: snapshot.revision, content: fixture.report[language], encoding: 'utf-8', lineEnding: 'lf', operationId: 'site-report-initialize', force: false });
    const assertScope = (projectId: string, workspacePath?: string | null) => { if (projectId !== locator.projectId || (workspacePath && workspacePath !== root)) fail(); };
    const relative = (canonicalPath: string) => {
      const file = fixture.files.find(file => `${root}/${file.path}` === canonicalPath);
      if (!file) return fail();
      return file.path;
    };
    const listeners = new Set<(event: WorkspaceFileChangedEventVm) => void>();
    const sourceSnapshot = async (state: z.infer<typeof statusSchema>): Promise<GitSourceControlSnapshotVm> => {
      const base = await original.getSourceControlSnapshot(locator.projectId, root);
      const rows = (area: 'index' | 'worktree') => state.changes.filter(row => ![' ', '?'].includes(row[area])).map(row => ({
        path: row.path, kind: row[area] === 'A' ? 'added' : 'modified', indexStatus: row.index.trim() || null,
        worktreeStatus: row.worktree.trim() || null, binary: false, submodule: false,
      } satisfies GitFileChangeVm));
      return { repository: { ...base.repository, repoRoot: root, commonDir: `${root}/.git`, workspacePath: root, headOid: state.head, currentBranch: 'preview',
        remotes: [], upstream: null, revision: state.revision },
      status: { snapshotRevision: state.revision, branch: { oid: state.head, head: 'preview', ahead: 0, behind: 0 }, conflicts: [], staged: rows('index'), unstaged: rows('worktree'),
        untracked: state.changes.filter(row => row.index === '?').map(row => ({ path: row.path, kind: 'untracked', worktreeStatus: '?', binary: false, submodule: false })) },
      refs: [{ fullName: 'refs/heads/preview', shortName: 'preview', kind: 'local-branch', targetOid: state.head, checkedOutWorktreePaths: [root] }],
      worktrees: [{ path: root, headOid: state.head, branch: 'refs/heads/preview', main: true, detached: false, locked: false, prunable: false, ownership: 'user' }], stashes: [] };
    };
    const api: Partial<RuntimeApi> = {
      async getGitCapability(projectId) {
        assertScope(projectId ?? locator.projectId);
        const state = statusSchema.parse(await request({ action: 'status' }));
        return { status: 'ready', installedVersion: state.gitVersion, minimumVersion: '2.36.0', repoRoot: root, commonDir: `${root}/.git`, head: state.head };
      },
      async getGitBranchPickerSnapshot(projectId, workspacePath) {
        assertScope(projectId, workspacePath);
        const state = statusSchema.parse(await request({ action: 'status' }));
        return { workspacePath: root, currentBranch: 'preview', headOid: state.head, revision: state.revision,
          dirtyFileCount: state.changes.length, operationInProgress: null, lock: { locked: false },
          branches: [{ name: 'preview', targetOid: state.head, checkedOutWorktreePaths: [root] }] };
      },
      async changeGitBranch() { return fail(); },
      async getTurnFileChangeSet(input, id) {
        const keys = ['projectId', 'taskId', 'runId', 'roundId', 'nodeId', 'attemptId', 'branchId', 'outerNodeId', 'outerAttemptId'] as const;
        if (id !== changeSetId || keys.some(key => (input[key] ?? null) !== (locator[key] ?? null))) fail();
        return structuredClone(changes);
      },
      async getFileComparison(input, id, changeId) {
        await api.getTurnFileChangeSet!(input, id);
        const change = changes.changes.find(change => change.id === changeId);
        const file = captures.find(file => file.path === change?.logicalPath);
        if (!file) return fail();
        const version = async (content: string, side: string) => {
          const bytes = new TextEncoder().encode(content);
          const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
          return { version: { id: `${changeId}-${side}`, storageKind: 'capturedBlob' as const,
            contentHash: [...digest].map(byte => byte.toString(16).padStart(2, '0')).join(''), byteLength: bytes.length, encoding: 'utf-8', lineEnding: 'lf' }, content };
        };
        return { changeSetId: id, changeId, path: file.path, stats: file.stats, before: file.before === null ? null : await version(file.before, 'before'), after: file.after === null ? null : await version(file.after, 'after'), limitationCode: null };
      },
      async listWorkspaceDirectory(projectId, path) {
        assertScope(projectId);
        if (!['', 'src', 'docs'].includes(path)) fail();
        return path ? fixture.files.filter(file => file.path.startsWith(`${path}/`)).map(file => ({ name: file.path.split('/').at(-1)!, relativePath: file.path, canonicalPath: `${root}/${file.path}`, kind: 'file', hasChildren: false, byteLength: null, modifiedAtNs: null }))
          : ['docs', 'src'].map(name => ({ name, relativePath: name, canonicalPath: `${root}/${name}`, kind: 'directory', hasChildren: true, byteLength: null, modifiedAtNs: null }));
      },
      async resolveWorkspaceFileLink(projectId, href) {
        assertScope(projectId);
        const path = href.startsWith(`${root}/`) ? relative(href) : relative(`${root}/${href}`);
        return { locator: { projectId, canonicalPath: `${root}/${path}`, relativePath: path, scope: 'workspace' }, target: null, externalAccessGrant: null };
      },
      async readFileResource(projectId, path, token, preferSource) {
        assertScope(projectId);
        if (path === report.locator.canonicalPath) return original.readFileResource(projectId, path, token, preferSource);
        const file = fileSchema.parse(await request({ action: 'read', path: relative(path) }));
        return { kind: 'text', locator: { projectId, canonicalPath: path, relativePath: file.path, scope: 'workspace' }, name: file.path.split('/').at(-1)!,
          revision: revision(file), content: file.content, encoding: 'utf-8', language: file.path.endsWith('.json') ? 'json' : 'markdown', lineEnding: 'lf', editable: true, limitationCode: null, externalAccessGrant: null };
      },
      async writeFileResource(input) {
        assertScope(input.projectId);
        const file = fileSchema.parse(await request({ action: 'write', path: relative(input.canonicalPath), content: input.content, expectedHash: input.expectedRevision.contentHash }));
        for (const listener of listeners) listener({ projectId: input.projectId, canonicalPath: input.canonicalPath, kind: 'modified', revision: revision(file), operationId: input.operationId });
        return revision(file);
      },
      async subscribeWorkspaceFileChanges(listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
      async getSourceControlSnapshot(projectId, workspacePath) { assertScope(projectId, workspacePath); return sourceSnapshot(statusSchema.parse(await request({ action: 'status' }))); },
      async getGitComparison(projectId, source) {
        if (source.kind !== 'workspace') return fail();
        assertScope(projectId, source.workspacePath);
        const value = comparisonSchema.parse(await request({ action: 'comparison', path: source.path, area: source.area }));
        return { path: value.path, stats: value.stats, before: value.before === null ? null : { content: value.before }, after: value.after === null ? null : { content: value.after }, limitationCode: null };
      },
      async executeGitMutation(projectId, workspacePath, input) {
        assertScope(projectId, workspacePath);
        if (input.kind !== 'stage-paths' && input.kind !== 'commit') return fail();
        const state = statusSchema.parse(await request(input.kind === 'commit' ? { action: 'commit', subject: input.subject, expectedRevision: input.expectedRevision } : { action: 'stage', paths: input.paths, expectedRevision: input.expectedRevision }));
        const snapshot = await sourceSnapshot(state);
        return input.kind === 'commit' ? { scope: 'repository' } : { scope: 'workspace', status: snapshot.status, repositoryRevision: state.revision };
      },
      async startGitOperation() { return fail(); },
    };
    return { api, summary: changes.summary, dispose, evidence: () => request({ action: 'status' }) };
  } catch (error) { await dispose(); throw error; }
}
