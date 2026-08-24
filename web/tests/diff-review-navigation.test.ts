import { describe, expect, it } from 'vitest';
import { gitComparisonReviewItemId, resolveDiffReviewNavigation, resolveReviewComparisonStats } from '@/components/workspace/source-control/diff-review-store';
import { diffTypePresentation } from '@/components/workspace/source-control/SourceControlDiffFileRow';
import { workspaceReviewItems } from '@/components/workspace/source-control/SourceControlWorkspacePanel';
import { pullRequestReviewItems } from '@/components/workspace/source-control/SourceControlGitHubView';
import {
  HISTORY_LIST_MIN_WIDTH,
  HISTORY_REVIEW_MIN_WIDTH,
  HISTORY_SPLIT_MIN_WIDTH,
  commitReviewPathCounts,
  restoreReviewScrollPosition,
} from '@/components/workspace/source-control/SourceControlHistoryView';
import { reduceFileWorkspaceResponsiveState } from '@/components/workspace/workspace-layout';
import type { GitCommitReviewFileVm } from '@/types';

describe('diff review navigation', () => {
  it('reapplies the review scroll position after the viewport finishes layout', () => {
    const viewport = { scrollTop: 0 };
    let deferredApply: (() => void) | null = null;
    const frame = restoreReviewScrollPosition(viewport, 288, (apply) => {
      deferredApply = apply;
      return 7;
    });

    expect(viewport.scrollTop).toBe(288);
    viewport.scrollTop = 0;
    (deferredApply as (() => void) | null)?.();
    expect(viewport.scrollTop).toBe(288);
    expect(frame).toBe(7);
  });

  it('uses the readable master-detail minimum as the history split boundary', () => {
    expect(HISTORY_SPLIT_MIN_WIDTH).toBeGreaterThanOrEqual(HISTORY_LIST_MIN_WIDTH + HISTORY_REVIEW_MIN_WIDTH);
    expect(reduceFileWorkspaceResponsiveState(
      { split: false, widthAtTransition: 0 },
      HISTORY_SPLIT_MIN_WIDTH - 1,
      HISTORY_SPLIT_MIN_WIDTH,
    ).split).toBe(false);
    expect(reduceFileWorkspaceResponsiveState(
      { split: false, widthAtTransition: 0 },
      HISTORY_SPLIT_MIN_WIDTH,
      HISTORY_SPLIT_MIN_WIDTH,
    ).split).toBe(true);
  });

  it('moves between chunks before crossing into the next file', () => {
    expect(resolveDiffReviewNavigation({ itemIndex: 0, itemCount: 3, chunkIndex: 0, chunkCount: 2, direction: 1 }))
      .toEqual({ kind: 'chunk', index: 1 });
    expect(resolveDiffReviewNavigation({ itemIndex: 0, itemCount: 3, chunkIndex: 1, chunkCount: 2, direction: 1 }))
      .toEqual({ kind: 'file', offset: 1, landing: 'first' });
  });

  it('moves to the previous file at its last chunk and stops at session boundaries', () => {
    expect(resolveDiffReviewNavigation({ itemIndex: 1, itemCount: 3, chunkIndex: 0, chunkCount: 2, direction: -1 }))
      .toEqual({ kind: 'file', offset: -1, landing: 'last' });
    expect(resolveDiffReviewNavigation({ itemIndex: 0, itemCount: 3, chunkIndex: 0, chunkCount: 2, direction: -1 }))
      .toEqual({ kind: 'none' });
    expect(resolveDiffReviewNavigation({ itemIndex: 2, itemCount: 3, chunkIndex: 1, chunkCount: 2, direction: 1 }))
      .toEqual({ kind: 'none' });
  });

  it('identifies independent topology chains that end at the same path in one pass', () => {
    const files: GitCommitReviewFileVm[] = [
      reviewFile('src/commands.rs', 'd12b9cd9'),
      reviewFile('src/commands.rs', '0cf78b22'),
      reviewFile('src/lib.rs', '870e077b'),
    ];

    expect([...commitReviewPathCounts(files)]).toEqual([
      ['src/commands.rs', 2],
      ['src/lib.rs', 1],
    ]);
  });

  it('uses one A/M/D presentation contract across source-control file lists', () => {
    expect(diffTypePresentation('untracked').label).toBe('A');
    expect(diffTypePresentation('added').label).toBe('A');
    expect(diffTypePresentation('deleted').label).toBe('D');
    expect(diffTypePresentation('renamed').label).toBe('M');
    expect(diffTypePresentation('copied').label).toBe('M');
    expect(diffTypePresentation('modified').className).toContain('blue');
  });

  it('uses the review-list summary as the Diff tab authority', () => {
    expect(resolveReviewComparisonStats(
      { addedLines: 230, deletedLines: 47 },
      { addedLines: 229, deletedLines: 46 },
    )).toEqual({ addedLines: 230, deletedLines: 47 });
    expect(resolveReviewComparisonStats(
      { addedLines: null, deletedLines: null },
      { addedLines: 12, deletedLines: 3 },
    )).toEqual({ addedLines: 12, deletedLines: 3 });
  });

  it('builds one immutable review sequence for all files in a PR', () => {
    const detail = {
      number: 42, baseRefOid: '1'.repeat(40), headRefOid: '2'.repeat(40),
      files: [
        { path: 'src/a.ts', oldPath: null, kind: 'modified' as const, additions: 2, deletions: 1 },
        { path: 'src/b.ts', oldPath: null, kind: 'added' as const, additions: 4, deletions: 0 },
      ],
    } as import('@/types').GitHubPullRequestDetailVm;
    const items = pullRequestReviewItems('D:/repo', 'github.com', 'acme/widgets', detail);
    expect(items).toHaveLength(2);
    expect(items[0].source).toMatchObject({ kind: 'github-pr', prNumber: 42, path: 'src/a.ts', baseOid: detail.baseRefOid, headOid: detail.headRefOid });
    expect(items[0].stats).toEqual({ addedLines: 2, deletedLines: 1 });
    expect(items[1].path).toBe('src/b.ts');
  });

  it('builds a stable workspace review sequence without loading diff content', () => {
    const items = workspaceReviewItems('D:/repo', 'unstaged', [{
      path: 'src/app.ts', oldPath: null, kind: 'modified', indexStatus: null,
      worktreeStatus: 'M', binary: false, submodule: false, addedLines: 2, deletedLines: 1,
    }]);
    expect(items).toEqual([{
      id: gitComparisonReviewItemId(items[0].source),
      path: 'src/app.ts',
      source: { kind: 'workspace', workspacePath: 'D:/repo', path: 'src/app.ts', area: 'unstaged' },
      stats: { addedLines: 2, deletedLines: 1 },
    }]);
  });
});

function reviewFile(path: string, afterOid: string): GitCommitReviewFileVm {
  return {
    path,
    oldPath: null,
    kind: 'modified',
    binary: false,
    beforeOid: `${afterOid}-parent`,
    beforePath: path,
    afterOid,
  };
}
