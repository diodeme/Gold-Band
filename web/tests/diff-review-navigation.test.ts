import { describe, expect, it } from 'vitest';
import { resolveDiffReviewNavigation } from '@/components/workspace/source-control/diff-review-store';
import {
  HISTORY_LIST_MIN_WIDTH,
  HISTORY_REVIEW_MIN_WIDTH,
  HISTORY_SPLIT_MIN_WIDTH,
  commitReviewPathCounts,
} from '@/components/workspace/source-control/SourceControlHistoryView';
import { reduceFileWorkspaceResponsiveState } from '@/components/workspace/workspace-layout';
import type { GitCommitReviewFileVm } from '@/types';

describe('diff review navigation', () => {
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
