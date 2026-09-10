import { afterEach, expect, it, vi } from 'vitest';
import { browserApi } from '../src/api/browser';
import { createReviewApi } from '../../marketing/site/review-api';
import { createReviewRepository } from '../../scripts/site-review-repository.mjs';

afterEach(() => vi.unstubAllGlobals());

it('projects capability and branch selection from the same isolated Git snapshot', async () => {
  const state = { root: '/temporary/workspace', initialHead: 'a'.repeat(40), head: 'b'.repeat(40), gitVersion: '2.53.0', revision: 'revision-1',
    changes: [{ path: 'src/config.json', index: ' ', worktree: 'M' }, { path: 'docs/workspace-notes.md', index: '?', worktree: '?' }] };
  vi.stubGlobal('fetch', vi.fn(async (_url, options) => {
    const body = JSON.parse(options.body);
    const result = body.action === 'initialize' ? { session: 'review-test' }
      : body.action === 'close' ? {} : body.command.action === 'status' ? state
      : { path: body.command.path, before: null, after: 'content\n', stats: { addedLines: 1, deletedLines: 0 } };
    return { ok: true, json: async () => result };
  }));
  const review = await createReviewApi(browserApi, 'en', { projectId: 'default', taskId: 'mock-task', runId: 'run-052', roundId: 'round-001', nodeId: 'dev', attemptId: 'attempt-001', branchId: 'root' });
  try {
    const api = { ...browserApi, ...review.api };
    const snapshot = await api.getSourceControlSnapshot('default');
    expect(await api.getGitCapability('default')).toMatchObject({ status: 'ready', repoRoot: snapshot.repository.repoRoot, head: state.head });
    expect(await api.getGitBranchPickerSnapshot('default')).toMatchObject({ currentBranch: 'preview', headOid: state.head, revision: state.revision, dirtyFileCount: 2,
      branches: [{ name: 'preview', targetOid: state.head, checkedOutWorktreePaths: ['/default'] }] });
    await expect(api.getGitCapability('other')).rejects.toMatchObject({ code: 'site.review-operation-rejected' });
  } finally { await review.dispose(); }
});

it('keeps captured Diff immutable while latest file edits flow through index and commit', async () => {
  const repository = await createReviewRepository('en');
  vi.stubGlobal('fetch', vi.fn(async (_url, options) => {
    const body = JSON.parse(options.body);
    try {
      const result = body.action === 'initialize' ? { session: 'real-test' }
        : body.action === 'close' ? await repository.dispose() : await repository.run(body.command);
      return { ok: true, json: async () => result ?? {} };
    } catch (error) { return { ok: false, json: async () => error }; }
  }));
  const locator = { projectId: 'default', taskId: 'mock-task', runId: 'run-052', roundId: 'round-001', nodeId: 'dev', attemptId: 'attempt-001', branchId: 'root' };
  const review = await createReviewApi(browserApi, 'en', locator);
  try {
    const api = { ...browserApi, ...review.api };
    const set = await api.getTurnFileChangeSet(locator, 'browser-change-set-052');
    const change = set.changes.find(row => row.logicalPath === 'docs/workspace-notes.md')!;
    const capture = await api.getFileComparison(locator, set.id, change.id);
    const file = await api.readFileResource('default', '/default/docs/workspace-notes.md');
    expect(file.kind).toBe('text');
    if (file.kind !== 'text') throw new Error('Expected text file');
    const updates = vi.fn();
    const unsubscribe = await api.subscribeWorkspaceFileChanges(updates);
    const content = `${file.content}\nReviewed locally.\n`;
    const revision = await api.writeFileResource({ projectId: 'default', canonicalPath: file.locator.canonicalPath,
      content, expectedRevision: file.revision, operationId: 'review-edit', encoding: 'utf-8', lineEnding: 'lf', force: false });
    expect(updates).toHaveBeenCalledWith(expect.objectContaining({ canonicalPath: file.locator.canonicalPath, operationId: 'review-edit', revision }));
    unsubscribe();
    expect(await api.getFileComparison(locator, set.id, change.id)).toEqual(capture);
    expect((await api.getGitComparison('default', { kind: 'workspace', workspacePath: '/default', path: change.logicalPath, area: 'unstaged' })).after?.content).toBe(content);
    const initial = await api.getSourceControlSnapshot('default');
    await api.executeGitMutation('default', null, { kind: 'stage-paths', paths: ['src/config.json', change.logicalPath], expectedRevision: initial.repository.revision });
    await expect(api.executeGitMutation('default', null, { kind: 'commit', subject: 'Stale', expectedRevision: initial.repository.revision })).rejects.toMatchObject({ code: 'git.snapshot-stale' });
    const staged = await api.getSourceControlSnapshot('default');
    expect(staged.status.staged).toHaveLength(2);
    await api.executeGitMutation('default', null, { kind: 'commit', subject: 'Review workspace changes', expectedRevision: staged.repository.revision });
    const committed = await api.getSourceControlSnapshot('default');
    expect(committed.repository.headOid).not.toBe(initial.repository.headOid);
    expect([...committed.status.staged, ...committed.status.unstaged, ...committed.status.untracked]).toEqual([]);
    await expect(api.getTurnFileChangeSet({ ...locator, outerNodeId: 'other' }, set.id)).rejects.toMatchObject({ code: 'site.review-operation-rejected' });
    await expect(api.readFileResource('other', file.locator.canonicalPath)).rejects.toMatchObject({ code: 'site.review-operation-rejected' });
  } finally { await review.dispose(); }
});
