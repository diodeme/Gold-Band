/** @vitest-environment jsdom */
import { act, useSyncExternalStore } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { SourceControlStore } from '@/components/workspace/source-control/source-control-store';
import { SourceControlDiffFileRow } from '@/components/workspace/source-control/SourceControlDiffFileRow';
import type { GitSourceControlSnapshotVm, WorkspaceFileChangedEventVm } from '@/types';

it('keeps the surviving row and its +/- DOM throughout watcher statistics revalidation', async () => {
  vi.useFakeTimers();
  const row = { path: 'keep.ts', kind: 'modified' as const, binary: false, submodule: false, addedLines: 9, deletedLines: 2 };
  const snapshot: GitSourceControlSnapshotVm = {
    catalogRevision: 'one',
    repository: { projectId: 'p', repoRoot: 'D:/repo', workspacePath: 'D:/repo', commonDir: 'D:/repo/.git', headOid: 'head', currentBranch: 'main', detached: false, unborn: false, remotes: [], lock: { locked: false }, revision: 'one', syncRevision: 'one' },
    status: { snapshotRevision: 'one', branch: { oid: 'head', head: 'main', ahead: 0, behind: 0 }, staged: [], unstaged: [row], untracked: [], conflicts: [] },
    refs: [], worktrees: [], stashes: [],
  };
  let emit!: (event: WorkspaceFileChangedEventVm) => void;
  let complete!: (status: typeof snapshot.status) => void;
  const pending = new Promise<typeof snapshot.status>((resolve) => { complete = resolve; });
  const getSnapshot = vi.fn().mockResolvedValue(snapshot);
  const store = new SourceControlStore({
    getCapability: vi.fn().mockResolvedValue({ status: 'ready' }), getSnapshot,
    getStatistics: vi.fn().mockResolvedValueOnce(snapshot.status).mockReturnValueOnce(pending),
    getCatalog: vi.fn(), getHistory: vi.fn(), getCommitReview: vi.fn(), getCommitReachability: vi.fn(),
    executeMutation: vi.fn(), startOperation: vi.fn(), cancelOperation: vi.fn(), initializeRepository: vi.fn(),
    subscribeWorkspaceChanges: async (listener) => { emit = listener; return () => {}; },
  });
  const subscribe = (listener: () => void) => store.subscribe('p', 'D:/repo', listener);
  const read = () => store.session('p', 'D:/repo');
  function View() {
    const session = useSyncExternalStore(subscribe, read);
    return session.snapshot?.status.unstaged.map((change) => <SourceControlDiffFileRow key={change.path} {...change} onClick={() => {}} />);
  }
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  try {
    await store.ensureLoaded('p', 'D:/repo');
    await act(async () => root.render(<View />));
    const summary = container.querySelector('[data-source-control-diff-summary]');
    expect(summary?.textContent).toBe('+9-2');
    getSnapshot.mockResolvedValueOnce({ ...snapshot, status: { ...snapshot.status, unstaged: [{ ...row, addedLines: null, deletedLines: null }] } });
    await act(async () => {
      emit({ projectId: 'p', workspacePath: 'D:/repo', canonicalPath: 'D:/repo/keep.ts', kind: 'modified', revision: null, operationId: null });
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(container.querySelector('[data-source-control-diff-summary]')).toBe(summary);
    expect(summary?.textContent).toBe('+9-2');
    await act(async () => {
      complete({ ...snapshot.status, unstaged: [{ ...row, addedLines: 12 }] });
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(container.querySelector('[data-source-control-diff-summary]')).toBe(summary);
    expect(summary?.textContent).toBe('+12-2');
  } finally {
    await act(async () => root.unmount());
    container.remove(); store.clear('p', 'D:/repo'); vi.useRealTimers();
  }
});
