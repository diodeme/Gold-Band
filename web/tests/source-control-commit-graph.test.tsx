/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { CommitGraph } from '@/components/workspace/source-control/CommitGraph';
import {
  resolveCommitGraphBranch,
  toCommitGraphEntries,
  type CommitGraphEntry,
} from '@/components/workspace/source-control/commit-graph-model';
import type { GitCommitVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

class NoopResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

const GRAPH_PAGE_SIZE = 300;
const GRAPH_DOM_NODE_BUDGET = 25_000;
const GRAPH_RENDER_BUDGET_MS = new Map([
  [300, 5_000],
  [1_000, 5_000],
  [5_000, 5_000],
]);

afterEach(() => {
  document.body.replaceChildren();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe('source control commit graph adapter', () => {
  it('maps the Git history model without leaking renderer-specific fields upstream', () => {
    const sourceCommit = commit({
      sourceRef: 'refs/heads/feature/history',
      refs: [{ fullName: 'refs/tags/v1.0.0', shortName: 'v1.0.0', kind: 'tag' }],
    });
    const detachedTag = commit({
      oid: 'b'.repeat(40),
      sourceRef: null,
      refs: [{ fullName: 'refs/tags/v2.0.0', shortName: 'v2.0.0', kind: 'tag' }],
    });

    const entries = toCommitGraphEntries([sourceCommit, detachedTag], 'main');

    expect(entries[0]).toMatchObject({
      hash: sourceCommit.oid,
      branch: 'feature/history',
      parents: sourceCommit.parentOids,
      message: sourceCommit.subject,
      committerDate: sourceCommit.committer.timestamp,
      runtimeCheckpoint: false,
    });
    expect(entries[0]?.refs).not.toBe(sourceCommit.refs);
    expect(entries[1]?.branch).toBe('tags/v2.0.0');
    expect(resolveCommitGraphBranch(commit({ refs: [] }), 'main')).toBe('main');
  });

  it('keeps 300/1,000/5,000-commit DAGs bounded to one page and preserves external multi-selection', async () => {
    vi.stubGlobal('ResizeObserver', NoopResizeObserver);
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    let entries = largeHistory(300);
    const onToggleSelected = vi.fn();
    const onOpenCommit = vi.fn();

    try {
      for (const commitCount of GRAPH_RENDER_BUDGET_MS.keys()) {
        entries = largeHistory(commitCount);
        const startedAt = performance.now();
        await act(async () => {
          root.render(
            <CommitGraph
              entries={entries}
              currentBranch="main"
              page={0}
              selectedOids={new Set(['commit-000000'])}
              runtimeLabel="runtime"
              selectLabel={(entry) => `select ${entry.hash}`}
              formatTimestamp={(timestamp) => timestamp}
              onToggleSelected={onToggleSelected}
              onOpenCommit={onOpenCommit}
            />,
          );
        });
        const renderDurationMs = performance.now() - startedAt;
        expect(container.querySelectorAll('[data-commit-graph-row]')).toHaveLength(GRAPH_PAGE_SIZE);
        expect(container.querySelectorAll('*').length).toBeLessThan(GRAPH_DOM_NODE_BUDGET);
        expect(renderDurationMs).toBeLessThan(GRAPH_RENDER_BUDGET_MS.get(commitCount)!);
      }

      expect(container.querySelector('[data-commit-graph-row="commit-000000"]')?.getAttribute('data-selected')).toBe('true');

      const checkbox = container.querySelector<HTMLButtonElement>('[aria-label="select commit-000000"]');
      await act(async () => checkbox?.click());
      expect(onToggleSelected).toHaveBeenCalledWith('commit-000000');
      expect(onOpenCommit).not.toHaveBeenCalled();

      await act(async () => {
        root.render(
          <CommitGraph
            entries={entries}
            currentBranch="main"
            page={1}
            selectedOids={new Set(['commit-000000', 'commit-000300'])}
            runtimeLabel="runtime"
            selectLabel={(entry) => `select ${entry.hash}`}
            formatTimestamp={(timestamp) => timestamp}
            onToggleSelected={onToggleSelected}
            onOpenCommit={onOpenCommit}
          />,
        );
      });
      expect(container.querySelectorAll('[data-commit-graph-row]')).toHaveLength(300);
      expect(container.querySelector('[data-commit-graph-row="commit-000300"]')).not.toBeNull();
      expect(container.querySelector('[data-commit-graph-row="commit-000000"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  }, 20_000);
});

function commit(overrides: Partial<GitCommitVm> = {}): GitCommitVm {
  return {
    oid: 'a'.repeat(40),
    parentOids: ['c'.repeat(40)],
    subject: 'feat: history graph',
    body: 'body',
    author: { name: 'Ada', email: 'ada@example.com', timestamp: '2026-08-10T10:00:00Z' },
    committer: { name: 'Ada', email: 'ada@example.com', timestamp: '2026-08-10T10:01:00Z' },
    refs: [{ fullName: 'refs/heads/main', shortName: 'main', kind: 'local-branch' }],
    sourceRef: null,
    runtimeCheckpoint: false,
    ...overrides,
  };
}

function largeHistory(count: number): CommitGraphEntry[] {
  return Array.from({ length: count }, (_, index) => {
    const hash = `commit-${index.toString().padStart(6, '0')}`;
    const firstParent = index + 1 < count ? `commit-${(index + 1).toString().padStart(6, '0')}` : null;
    const mergeParent = index % 97 === 0 && index + 7 < count
      ? `commit-${(index + 7).toString().padStart(6, '0')}`
      : null;
    return {
      hash,
      branch: index % 97 < 7 ? `feature-${Math.floor(index / 97)}` : 'main',
      parents: [firstParent, mergeParent].filter((parent): parent is string => Boolean(parent)),
      message: `commit ${index}`,
      committerDate: '2026-08-10T10:01:00Z',
      author: { name: 'Ada', email: 'ada@example.com' },
      refs: index % 300 === 0
        ? [{ fullName: `refs/tags/page-${index / 300}`, shortName: `page-${index / 300}`, kind: 'tag' }]
        : [],
      runtimeCheckpoint: index % 113 === 0,
    };
  });
}
