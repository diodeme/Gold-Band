import { describe, expect, it, vi } from 'vitest';
import { GitHubComparisonCache } from '@/components/workspace/source-control/github-comparison-cache';
import type { GitFileComparisonVm } from '@/types';

describe('GitHub comparison cache', () => {
  it('caches PR comparisons by immutable base and head revision', async () => {
    const comparison: GitFileComparisonVm = {
      path: 'src/app.ts',
      stats: { addedLines: 1, deletedLines: 1 },
      before: { content: 'before' },
      after: { content: 'after' },
      limitationCode: null,
    };
    const getComparison = vi.fn().mockResolvedValue(comparison);
    const cache = new GitHubComparisonCache(getComparison);
    const source = {
      kind: 'github-pr' as const,
      workspacePath: 'D:/repo',
      host: 'github.com',
      repository: 'acme/widgets',
      prNumber: 42,
      baseOid: '1111111111111111111111111111111111111111',
      headOid: '2222222222222222222222222222222222222222',
      path: 'src/app.ts',
    };

    await cache.get('project-1', source);
    await cache.get('project-1', source);
    expect(getComparison).toHaveBeenCalledTimes(1);

    await cache.get('project-1', {
      ...source,
      headOid: '3333333333333333333333333333333333333333',
    });
    expect(getComparison).toHaveBeenCalledTimes(2);
  });

  it('deduplicates an in-flight comparison request', async () => {
    const pending = deferred<GitFileComparisonVm>();
    const getComparison = vi.fn().mockReturnValue(pending.promise);
    const cache = new GitHubComparisonCache(getComparison);
    const source = {
      kind: 'github-pr' as const,
      host: 'github.com',
      repository: 'acme/widgets',
      prNumber: 42,
      baseOid: '1111111111111111111111111111111111111111',
      headOid: '2222222222222222222222222222222222222222',
      path: 'src/app.ts',
    };

    const first = cache.get('project-1', source);
    const duplicate = cache.get('project-1', source);
    expect(duplicate).toBe(first);
    expect(getComparison).toHaveBeenCalledTimes(1);
    pending.resolve({
      path: source.path,
      stats: { addedLines: 1, deletedLines: 1 },
      before: { content: 'before' },
      after: { content: 'after' },
      limitationCode: null,
    });
    await first;
  });

  it('isolates renamed files by their base revision path', async () => {
    const getComparison = vi.fn().mockResolvedValue({ path: 'src/new.ts', stats: { addedLines: 1, deletedLines: 1 } });
    const cache = new GitHubComparisonCache(getComparison);
    const source = {
      kind: 'github-pr' as const, host: 'github.com', repository: 'acme/widgets', prNumber: 42,
      baseOid: '1'.repeat(40), headOid: '2'.repeat(40), path: 'src/new.ts', beforePath: 'src/old.ts',
    };
    await cache.get('project-1', source);
    await cache.get('project-1', { ...source, beforePath: 'src/other.ts' });
    expect(getComparison).toHaveBeenCalledTimes(2);
  });
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });
  return { promise, resolve, reject };
}
