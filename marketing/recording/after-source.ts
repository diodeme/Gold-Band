import type { RuntimeApi } from '@/api/client';
import type { GitSourceControlSnapshotVm, TurnFileLocatorVm } from '@/types';
import { createPreviewRun, taskTitle } from '../shared/fixture';
import { createWorkflowSource } from './workflow-source';
import type { Language } from '../site/content';

const files = ['src/config.json', 'docs/workspace-notes.md'];
const reportPath = '/default/review.md';
const changeSetId = 'browser-change-set-052';
const fail = (code: string): never => { throw { code: `recording.after.${code}`, params: {} }; };
type Seed = { id: string; before: Record<string, string | null>; captured: Record<string, string>; report: string };
type Repository = { head: string; initialHead: string; revision: string; subject: string; changes: Array<{ path: string; index: string; worktree: string }> };

export function createAfterSource(base: RuntimeApi, language: Language): Partial<RuntimeApi> {
  async function request<T>(input: object): Promise<T> {
    const response = await fetch('/__recording/after', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(input) });
    const data = await response.json(); if (!response.ok) throw data.error; return data.result;
  }
  let seedPromise: Promise<Seed> | undefined;
  const seed = () => seedPromise ??= request<Seed>({ action: 'start', language });
  const call = async <T>(input: object) => request<T>({ ...input, id: (await seed()).id });
  const validate = (project: string, task = 'mock-task', run = 'run-052') => { if (project !== 'default' || task !== 'mock-task' || run !== 'run-052') fail('locator-invalid'); };
  const validateFiles = (locator: TurnFileLocatorVm, id: string) => {
    validate(locator.projectId, locator.taskId, locator.runId);
    if (locator.roundId !== 'round-001' || locator.nodeId !== 'continue' || locator.attemptId !== 'attempt-001' || locator.branchId !== 'root' || locator.outerNodeId || locator.outerAttemptId || id !== changeSetId) fail('locator-invalid');
  };
  const filePath = (canonicalPath: string) => { const path = canonicalPath.slice('/default/'.length); if (canonicalPath !== `/default/${path}` || !files.includes(path)) fail('path-invalid'); return path; };
  const version = (content: string) => ({ id: 'recording-captured', storageKind: 'capturedBlob' as const, contentHash: 'recording-captured', byteLength: new TextEncoder().encode(content).length, encoding: 'utf-8', lineEnding: 'lf' });
  // Prepare the existing story once, before capture; Git owns all subsequent review mutations.
  const source = createWorkflowSource(base, language);
  const ready = (async () => {
    const locator = ['default', 'mock-task', 'run-052', 'round-001', 'review', 'attempt-001'] as const;
    await source.advance('streaming'); await source.advance('streaming');
    await source.advance('question-wait');
    await source.api.respondElicitation!(...locator, 'snapshot-question', 'accept', { answer: language === 'zh' ? '每轮保留' : 'Every turn' });
    await source.advance('permission-wait');
    await source.api.respondAcpPermission!(...locator, 'write-config', 'allow-once');
    await source.advance('result-false'); await source.advance('branch');
    await source.advance('result-true'); await source.advance('branch');
    const value = await source.snapshot();
    const completed = createPreviewRun(value, language);
    Object.assign(value, { runStatus: 'completed', runOutcome: 'success', activeSessions: [], selectedSession: completed.selectedSession });
    value.selectedSession!.events = value.selectedSession!.events.map(event => event.kind === 'fileChangeSet' ? { ...event, raw: { ...event.raw as object, attachmentCount: 1 } } : event);
    value.selectedSession!.events = value.selectedSession!.events.filter(event => event.id !== 'site-plan').map((event, index) => ({ ...event, seq: index + 1,
      ...(event.id === 'site-user' ? { content: language === 'zh' ? '完善工作区配置与说明，保留文件快照。' : 'Update workspace configuration and docs; retain file snapshots.' } : {}),
      ...(event.id === 'site-result' ? { content: language === 'zh' ? '配置与说明已更新，请审阅报告和文件变更。' : 'Configuration and docs updated. Review the report and file changes.' } : {}) }));
    const count = value.selectedSession!.events.length;
    Object.assign(value.selectedSession!.eventPage, { loadedCount: count, total: count, newestSeq: count });
    value.selectedSession!.diagnostics.eventCount = count;
    value.selectedSession!.worktreePath = null; value.selectedSession!.worktreeBranch = null;
    const round = value.sessionTree.rounds[0];
    const final = round.nodes.find(node => node.nodeId === 'continue')!;
    const terminal = completed.sessionTree.rounds[0].nodes.find(node => node.nodeId === 'continue')!;
    Object.assign(final, { status: terminal.status, runtimeDisplay: terminal.runtimeDisplay });
    Object.assign(final.attempts[0], terminal.attempts[0], { current: false, attachmentCount: 1 });
    for (const node of round.nodes) for (const attempt of node.attempts) {
      attempt.current = false; attempt.lifecycle!.runtime.current = false; attempt.lifecycle!.runtime.active = false;
    }
    Object.assign(round, { status: 'completed', runtimeDisplay: terminal.runtimeDisplay });
    value.workflowGraph.nodes = value.workflowGraph.nodes.map(node => node.id === 'continue'
      ? { ...node, status: 'completed', outcome: 'success', runtimeDisplay: terminal.runtimeDisplay, current: false, attachmentCount: 1 }
      : { ...node, current: false });
    source.dispose();
    return value;
  })();
  const run = async (selected?: string | null) => {
    const value = structuredClone(await ready);
    if (selected && selected !== value.sessionTree.selectedSessionKey) {
      const node = value.sessionTree.rounds[0].nodes.find(node => selected === `round-001/${node.nodeId}/attempt-001`);
      if (!node) return fail('locator-invalid');
      value.selectedSession = await source.api.getAcpSession!('default', 'mock-task', 'run-052', 'round-001', node.nodeId, 'attempt-001');
      value.sessionTree.selectedSessionKey = selected;
    }
    return value;
  };
  const session = async (p: string | null | undefined, t: string, r: string, round: string, node: string, attempt?: string | null, outer?: string | null, outerAttempt?: string | null) => {
    validate(p ?? '', t, r);
    if (round !== 'round-001' || (attempt && attempt !== 'attempt-001') || outer || outerAttempt) return fail('locator-invalid');
    return (await run(`round-001/${node}/attempt-001`)).selectedSession!;
  };
  async function sourceSnapshot(): Promise<GitSourceControlSnapshotVm> {
    const [template, actual] = await Promise.all([base.getSourceControlSnapshot('default', null), call<Repository>({ action: 'snapshot' })]);
    const change = (row: Repository['changes'][number]) => ({ path: row.path, oldPath: null, kind: row.index === '?' ? 'untracked' as const : row.index === 'A' ? 'added' as const : 'modified' as const, indexStatus: row.index.trim() || null, worktreeStatus: row.worktree.trim() || null, binary: false, submodule: false, addedLines: null, deletedLines: null });
    const branch = { oid: actual.head, head: 'review/workspace', upstream: null, ahead: 0, behind: 0 };
    return { ...template, repository: { ...template.repository, repoRoot: '/default', commonDir: '/default/.git', workspacePath: '/default', headOid: actual.head, currentBranch: branch.head, upstream: null, remotes: [], revision: actual.revision },
      status: { ...template.status, snapshotRevision: actual.revision, branch, conflicts: [], staged: actual.changes.filter(row => ![' ', '?'].includes(row.index)).map(change), unstaged: actual.changes.filter(row => ![' ', '?'].includes(row.worktree)).map(change), untracked: actual.changes.filter(row => row.index === '?').map(change), operationInProgress: null },
      refs: [{ ...template.refs[0], fullName: 'refs/heads/review/workspace', shortName: branch.head, targetOid: actual.head, upstream: null, ahead: 0, behind: 0, checkedOutWorktreePaths: ['/default'] }],
      worktrees: [{ ...template.worktrees[0], path: '/default', headOid: actual.head, branch: 'refs/heads/review/workspace' }], stashes: [] };
  }
  return {
    async getConversationRun(p, t, r, selected) { validate(p, t, r); return run(selected); },
    async getAcpSession(p, t, r, round, node, attempt, query, _fallback, outer, outerAttempt) { if (query?.branchId && query.branchId !== 'root') fail('locator-invalid'); return session(p, t, r, round, node, attempt, outer, outerAttempt); },
    async getAcpActivityDetail(p, t, r, round, node, attempt, _query, outer, outerAttempt) { return { items: (await session(p, t, r, round, node, attempt, outer, outerAttempt)).events.filter(event => event.kind === 'toolCall'), hasMoreEarlier: false, earlierCursor: null }; },
    async getAcpToolDetail(p, t, r, round, node, attempt, query, outer, outerAttempt) { return { event: (await session(p, t, r, round, node, attempt, outer, outerAttempt)).events.find(event => event.toolCallId === query.toolCallId) ?? null }; },
    getWorkflow: source.api.getWorkflow,
    showArtifact: source.api.showArtifact,
    async getConversationTaskPage(projectId) { validate(projectId); const value = await run(); return { projectId, tasks: [{ projectId, taskId: value.taskId, taskUuid: value.taskUuid, title: taskTitle(language), autoTitle: false, runMode: 'workflow', lastActivityAt: value.selectedSession!.sessionUpdatedAt!, runs: [], runHistoryStatus: 'ready-empty', runsNextCursor: null, pinned: false }], nextCursor: null, errors: [] }; },
    async getTurnFileChangeSet(locator, id) {
      validateFiles(locator, id); const data = await seed();
      return { id, turnId: 'site-tool', promptEventId: 'site-user', branchId: 'root', status: 'finalized', startedAt: (await run()).selectedSession!.sessionStartedAt!, finishedAt: null,
        summary: { fileCount: 2, addedFiles: 1, modifiedFiles: 1, deletedFiles: 0, addedLines: 5, deletedLines: 1 },
        changes: files.map((path, index) => ({ id: path, changeKind: index ? 'added' : 'modified', logicalPath: path, text: true, addedLines: index ? 4 : 1, deletedLines: index ? 0 : 1 })),
        attachments: [{ id: 'review-report', relativePath: 'review.md', name: 'review.md', byteLength: new TextEncoder().encode(data.report).length }], limitationCodes: [] };
    },
    async getFileComparison(locator, id, path) {
      validateFiles(locator, id); if (!files.includes(path)) fail('path-invalid'); const data = await seed(); const before = data.before[path], after = data.captured[path];
      return { changeSetId: id, changeId: path, path, stats: { addedLines: before ? 1 : 4, deletedLines: before ? 1 : 0 }, before: before ? { content: before, version: version(before) } : null, after: { content: after, version: version(after) }, limitationCode: null };
    },
    async resolveTurnAttachmentFile(locator, id, attachment) { validateFiles(locator, id); if (attachment !== 'review-report') fail('path-invalid'); return { locator: { projectId: 'default', canonicalPath: reportPath, relativePath: 'review.md', scope: 'workspace' }, target: null, externalAccessGrant: null }; },
    async listWorkspaceDirectory(project, path = '') { validate(project); if (!['', 'src', 'docs'].includes(path)) fail('path-invalid');
      return (path ? files.filter(file => file.startsWith(`${path}/`)) : ['src', 'docs']).map(file => ({ name: file.split('/').at(-1)!, relativePath: file, canonicalPath: `/default/${file}`, kind: path ? 'file' : 'directory', hasChildren: !path, byteLength: null, modifiedAtNs: null })); },
    async readFileResource(project, canonicalPath) {
      validate(project); const report = canonicalPath === reportPath;
      const data = report ? { content: (await seed()).report, revision: { byteLength: new TextEncoder().encode((await seed()).report).length, modifiedAtNs: '0', contentHash: 'report' } } : await call<{ content: string; revision: { contentHash: string; byteLength: number; modifiedAtNs: string } }>({ action: 'read', path: filePath(canonicalPath) });
      return { kind: 'text', locator: { projectId: project, canonicalPath, relativePath: canonicalPath.slice('/default/'.length), scope: 'workspace' }, name: canonicalPath.split('/').at(-1)!, ...data, encoding: 'utf-8', language: canonicalPath.endsWith('.json') ? 'json' : 'markdown', lineEnding: 'lf', editable: !report, limitationCode: null, externalAccessGrant: null };
    },
    async writeFileResource(input) { validate(input.projectId); return call({ action: 'save', path: filePath(input.canonicalPath), content: input.content, expectedHash: input.expectedRevision.contentHash }); },
    async getSourceControlSnapshot(project) { validate(project); return sourceSnapshot(); },
    async getGitBranchPickerSnapshot(project) {
      validate(project);
      const [template, actual] = await Promise.all([base.getGitBranchPickerSnapshot(project), call<Repository>({ action: 'snapshot' })]);
      return { ...template, workspacePath: '/default', currentBranch: 'review/workspace', headOid: actual.head, revision: actual.revision, dirtyFileCount: actual.changes.length, operationInProgress: null,
        branches: [{ name: 'review/workspace', targetOid: actual.head, checkedOutWorktreePaths: ['/default'] }] };
    },
    async getGitComparison(project, source) { validate(project); if (source.kind !== 'workspace') return fail('operation-unavailable'); const data = await call<{ before: string | null; after: string | null; stats: { addedLines: number; deletedLines: number } }>({ action: 'comparison', path: source.path, area: source.area }); return { path: source.path, before: data.before === null ? null : { content: data.before }, after: data.after === null ? null : { content: data.after }, stats: data.stats, limitationCode: null }; },
    async executeGitMutation(project, _workspace, input) { validate(project); await call({ action: 'mutate', input }); const snapshot = await sourceSnapshot(); return { scope: 'workspace', status: snapshot.status, repositoryRevision: snapshot.repository.revision }; },
  };
}
