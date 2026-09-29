import { beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  getGitComparison: vi.fn(),
}));

vi.mock('@/api', () => api);

import { diffReviewStore, gitDiffReviewItemId, shouldRetainVisibleComparison, workspaceReviewItems } from '@/components/workspace/source-control/diff-review-store';
import type { GitFileComparisonVm } from '@/types';

function comparison(content: string): GitFileComparisonVm {
  return {
    path: 'src/accept.md',
    stats: { addedLines: 7, deletedLines: 6 },
    before: { content: 'before' },
    after: { content },
    limitationCode: null,
  };
}

function workspaceItem(stats: { addedLines: number | null; deletedLines: number | null } = { addedLines: 7, deletedLines: 6 }) {
  return workspaceReviewItems('D:/repo', 'unstaged', [{
    path: 'src/accept.md',
    oldPath: null,
    kind: 'modified',
    indexStatus: null,
    worktreeStatus: 'M',
    binary: false,
    submodule: false,
    addedLines: stats.addedLines,
    deletedLines: stats.deletedLines,
  }])[0];
}

beforeEach(() => {
  diffReviewStore.clearForTests();
  api.getGitComparison.mockReset();
});

describe('workspace diff review cache', () => {
  it('merges late commit statistics only into the matching workspace without refetching content', async () => {
    const file = { path: 'src/accept.md', beforePath: 'src/accept.md', beforeOid: 'before', afterOid: 'after', kind: 'modified' as const, binary: false, addedLines: 12, deletedLines: 3 };
    const item = { id: gitDiffReviewItemId(file.afterOid, file.beforeOid, file.beforePath, file.path), path: file.path,
      source: { ...file, kind: 'commit' as const, workspacePath: 'D:/repo' }, stats: { addedLines: null as number | null, deletedLines: null as number | null } };
    diffReviewStore.save({ id: 'review', projectId: 'project-1', revision: 'revision', items: [item] });
    api.getGitComparison.mockResolvedValue(comparison('immutable'));
    const pending = diffReviewStore.comparison('project-1', item);
    diffReviewStore.publishCommitStatistics('project-1', 'D:/other', [file]);
    expect(item.stats.addedLines).toBeNull();
    const notify = vi.fn();
    const unsubscribe = diffReviewStore.subscribe(notify);
    diffReviewStore.publishCommitStatistics('project-1', 'D:/repo', [file]);
    expect(item.stats).toEqual({ addedLines: 12, deletedLines: 3 });
    expect(diffReviewStore.comparison('project-1', item)).toBe(pending);
    expect((await pending).stats).toEqual(item.stats);
    expect(api.getGitComparison).toHaveBeenCalledTimes(1);
    expect(notify).toHaveBeenCalledTimes(1);
    unsubscribe();
  });

  it('keeps the visible diff mounted while the same file is refreshed', () => {
    expect(shouldRetainVisibleComparison('src/accept.md', 'src/accept.md')).toBe(true);
    expect(shouldRetainVisibleComparison('src/accept.md', 'src/other.md')).toBe(false);
    expect(shouldRetainVisibleComparison(null, 'src/accept.md')).toBe(false);
  });

  it('refetches a workspace file after that path is invalidated and keeps an unchanged commit comparison', async () => {
    const item = workspaceItem();
    api.getGitComparison
      .mockResolvedValueOnce(comparison('previous'))
      .mockResolvedValueOnce(comparison('current'));
    diffReviewStore.save({
      id: 'project-1:workspace:D:/repo:unstaged:revision-1',
      projectId: 'project-1',
      revision: 'revision-1',
      workspace: { workspacePath: 'D:/repo', area: 'unstaged' },
      items: [item],
    });

    await expect(diffReviewStore.comparison('project-1', item).then((value) => value.after?.content)).resolves.toBe('previous');
    diffReviewStore.publishWorkspaceRefresh({
      projectId: 'project-1',
      workspacePath: 'D:/repo',
      staged: [],
      unstaged: [],
      untracked: [],
      invalidate: { all: false, paths: ['src/accept.md'] },
    });
    await expect(diffReviewStore.comparison('project-1', item).then((value) => value.after?.content)).resolves.toBe('current');
    expect(api.getGitComparison).toHaveBeenCalledTimes(2);

    const commitItem = {
      id: 'commit:abc::src/accept.md',
      path: 'src/accept.md',
      source: { kind: 'commit' as const, workspacePath: 'D:/repo', path: 'src/accept.md', beforeOid: null, beforePath: null, afterOid: 'abc' },
      stats: { addedLines: 1, deletedLines: 0 },
    };
    api.getGitComparison.mockResolvedValue(comparison('commit'));
    await diffReviewStore.comparison('project-1', commitItem);
    await diffReviewStore.comparison('project-1', commitItem);
    expect(api.getGitComparison).toHaveBeenCalledTimes(3);
  });

  it('updates the open review sequence in place when line stats change and drops a file that left the change list', () => {
    const item = workspaceItem();
    const sessionId = 'project-1:workspace:D:/repo:unstaged:revision-1';
    diffReviewStore.save({
      id: sessionId,
      projectId: 'project-1',
      revision: 'revision-1',
      workspace: { workspacePath: 'D:/repo', area: 'unstaged' },
      items: [item, workspaceReviewItems('D:/repo', 'unstaged', [{
        path: 'src/other.md', oldPath: null, kind: 'modified', indexStatus: null, worktreeStatus: 'M',
        binary: false, submodule: false, addedLines: 1, deletedLines: 1,
      }])[0]],
    });

    diffReviewStore.publishWorkspaceRefresh({
      projectId: 'project-1',
      workspacePath: 'd:\\repo',
      staged: [],
      unstaged: [{
        path: 'src/accept.md', oldPath: null, kind: 'modified', indexStatus: null, worktreeStatus: 'M',
        binary: false, submodule: false, addedLines: 7, deletedLines: 6,
      }],
      untracked: [],
      invalidate: { all: false, paths: [] },
    });

    const session = diffReviewStore.get(sessionId);
    expect(session?.id).toBe(sessionId);
    expect(session?.items.map((entry) => entry.path)).toEqual(['src/accept.md']);
    expect(session?.items[0]?.stats).toEqual({ addedLines: 7, deletedLines: 6 });
  });

  it('invalidates every cached workspace comparison when Git metadata changes, without a new review tab', async () => {
    const item = workspaceItem();
    api.getGitComparison
      .mockResolvedValueOnce(comparison('staged-before'))
      .mockResolvedValueOnce(comparison('staged-after'));
    diffReviewStore.save({
      id: 'project-1:workspace:main:staged:revision-1',
      projectId: 'project-1',
      revision: 'revision-1',
      workspace: { workspacePath: null, area: 'staged' },
      items: workspaceReviewItems(null, 'staged', [{
        path: 'src/accept.md', oldPath: null, kind: 'modified', indexStatus: 'M', worktreeStatus: null,
        binary: false, submodule: false, addedLines: 2, deletedLines: 0,
      }]),
    });
    const staged = diffReviewStore.get('project-1:workspace:main:staged:revision-1')?.items[0];
    if (!staged) throw new Error('missing staged item');
    await diffReviewStore.comparison('project-1', staged);

    diffReviewStore.publishWorkspaceRefresh({
      projectId: 'project-1',
      workspacePath: null,
      staged: [{
        path: 'src/accept.md', oldPath: null, kind: 'modified', indexStatus: 'M', worktreeStatus: null,
        binary: false, submodule: false, addedLines: 4, deletedLines: 1,
      }],
      unstaged: [],
      untracked: [],
      invalidate: { all: true, paths: [] },
    });

    expect(diffReviewStore.get('project-1:workspace:main:staged:revision-1')?.items[0]?.stats).toEqual({ addedLines: 4, deletedLines: 1 });
    await expect(diffReviewStore.comparison('project-1', staged).then((value) => value.after?.content)).resolves.toBe('staged-after');
    expect(api.getGitComparison).toHaveBeenCalledTimes(2);
    expect(item.source.area).toBe('unstaged');
  });
});
