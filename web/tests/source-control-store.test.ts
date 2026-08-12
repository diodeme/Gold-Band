import { describe, expect, it, vi } from 'vitest';
import { SourceControlStore } from '@/components/workspace/source-control/source-control-store';
import type {
  GitHistoryPageVm,
  GitOperationVm,
  GitSourceControlSnapshotVm,
} from '@/types';

describe('source control session store', () => {
  it('restores repository data and view state after a Diff tab round trip without reloading', async () => {
    const api = fakeApi();
    const store = new SourceControlStore(api);

    await store.ensureLoaded('project-1', 'D:/repo');
    store.setActiveTab('project-1', 'D:/repo', 'history');
    store.setHistoryPage('project-1', 'D:/repo', 2);
    store.toggleCommitSelection('project-1', 'D:/repo', 'commit-1');

    // Remounting SourceControlWorkspacePanel calls ensureLoaded again.
    await store.ensureLoaded('project-1', 'D:/repo');

    expect(api.getSnapshot).toHaveBeenCalledTimes(1);
    expect(api.getHistory).toHaveBeenCalledTimes(1);
    expect(store.session('project-1', 'D:/repo')).toMatchObject({
      status: 'ready',
      activeTab: 'history',
      historyPage: 2,
      canonicalWorkspacePath: 'D:/repo',
    });
    expect(store.session('project-1', 'D:/repo').selectedCommitOids.has('commit-1')).toBe(true);
  });

  it('reloads only on explicit refresh and resets stale history navigation', async () => {
    const api = fakeApi();
    const store = new SourceControlStore(api);
    await store.ensureLoaded('project-1', 'D:/repo');
    store.setHistoryPage('project-1', 'D:/repo', 3);
    store.toggleCommitSelection('project-1', 'D:/repo', 'commit-1');

    await store.refresh('project-1', 'D:/repo');

    expect(api.getSnapshot).toHaveBeenCalledTimes(2);
    expect(api.getHistory).toHaveBeenCalledTimes(2);
    expect(store.session('project-1', 'D:/repo')).toMatchObject({ historyPage: 0 });
    expect(store.session('project-1', 'D:/repo').selectedCommitOids.size).toBe(0);
  });

  it('uses a successful mutation snapshot and refreshes history immediately', async () => {
    const api = fakeApi();
    const store = new SourceControlStore(api);
    await store.ensureLoaded('project-1', 'D:/repo');
    api.executeMutation.mockResolvedValueOnce({ snapshot: repositorySnapshot('D:/repo', 'revision-2') });

    await store.mutate('project-1', 'D:/repo', { kind: 'stage-all' }, 'stage-all');

    expect(api.executeMutation).toHaveBeenCalledWith('project-1', 'D:/repo', {
      kind: 'stage-all',
      expectedRevision: 'revision-1',
    });
    expect(api.getHistory).toHaveBeenCalledTimes(2);
    expect(store.session('project-1', 'D:/repo').snapshot?.repository.revision).toBe('revision-2');
  });

  it('isolates sessions for separate worktrees and aliases the canonical Windows path', async () => {
    const api = fakeApi();
    api.getSnapshot
      .mockResolvedValueOnce(repositorySnapshot('D:/repo/worktree-a'))
      .mockResolvedValueOnce(repositorySnapshot('D:/repo/worktree-b'));
    const store = new SourceControlStore(api);

    await Promise.all([
      store.ensureLoaded('project-1', 'D:\\repo\\worktree-a\\'),
      store.ensureLoaded('project-1', 'D:/repo/worktree-b'),
    ]);
    store.setActiveTab('project-1', 'd:/REPO/worktree-a', 'github');

    expect(store.session('project-1', 'D:/repo/worktree-a').activeTab).toBe('github');
    expect(store.session('project-1', 'D:/repo/worktree-b').activeTab).toBe('changes');
    expect(api.getSnapshot).toHaveBeenCalledTimes(2);
  });

  it('prevents an older request from overwriting a newer refresh', async () => {
    const firstSnapshot = deferred<GitSourceControlSnapshotVm>();
    const firstHistory = deferred<GitHistoryPageVm>();
    const api = fakeApi();
    api.getSnapshot
      .mockReturnValueOnce(firstSnapshot.promise)
      .mockResolvedValueOnce(repositorySnapshot('D:/repo', 'revision-new'));
    api.getHistory
      .mockReturnValueOnce(firstHistory.promise)
      .mockResolvedValueOnce(historyPage('history-new'));
    const store = new SourceControlStore(api);

    const older = store.ensureLoaded('project-1', 'D:/repo');
    const newer = store.refresh('project-1', 'D:/repo');
    await newer;
    firstSnapshot.resolve(repositorySnapshot('D:/repo', 'revision-old'));
    firstHistory.resolve(historyPage('history-old'));
    await older;

    expect(store.session('project-1', 'D:/repo').snapshot?.repository.revision).toBe('revision-new');
    expect(store.session('project-1', 'D:/repo').history?.revision).toBe('history-new');
  });

  it('bounds inactive repository sessions with an LRU cache', async () => {
    const api = fakeApi();
    const store = new SourceControlStore(api);
    for (let index = 0; index <= SourceControlStore.MAX_SESSIONS; index += 1) {
      await store.ensureLoaded('project-1', `D:/repo/worktree-${index}`);
    }

    expect(store.session('project-1', 'D:/repo/worktree-0').status).toBe('idle');
    expect(store.session('project-1', `D:/repo/worktree-${SourceControlStore.MAX_SESSIONS}`).status).toBe('ready');
  });

  it('finishes long Git operations from events without polling and refreshes immediately', async () => {
    const events = eventApi();
    const queued: GitOperationVm = {
      operationId: 'operation-1',
      kind: 'fetch',
      repositoryCommonDir: 'D:/repo/.git',
      workspacePath: 'D:/repo',
      status: 'queued',
      cancelable: true,
      startedAt: null,
      completedAt: null,
      error: null,
    };
    events.api.startOperation.mockResolvedValueOnce(queued);
    const store = new SourceControlStore(events.api);
    await store.ensureLoaded('project-1', 'D:/repo');

    await store.startOperation('project-1', 'D:/repo', { kind: 'fetch', prune: true }, 'fetch');
    events.emitOperation({
      ...queued,
      status: 'succeeded',
      cancelable: false,
      completedAt: '2026-08-11T00:00:00.000Z',
    });

    await vi.waitFor(() => expect(store.session('project-1', 'D:/repo').pendingOperation).toBeNull());
    expect(events.api.getSnapshot).toHaveBeenCalledTimes(2);
    expect(events.api.getOperation).not.toHaveBeenCalled();
  });

  it('debounces matching repository events and preserves navigation during snapshot invalidation', async () => {
    vi.useFakeTimers();
    try {
      const events = eventApi();
      const store = new SourceControlStore(events.api);
      await store.ensureLoaded('project-1', 'D:/repo');
      store.setActiveTab('project-1', 'D:/repo', 'history');
      store.setHistoryPage('project-1', 'D:/repo', 4);

      events.emitState({
        projectId: 'project-1',
        repositoryCommonDir: 'd:\\REPO\\.git',
        workspacePath: 'd:\\REPO',
        reason: 'metadata',
      });
      events.emitState({
        projectId: 'project-1',
        repositoryCommonDir: 'D:/repo/.git',
        workspacePath: 'D:/repo',
        reason: 'metadata',
      });
      await vi.advanceTimersByTimeAsync(151);

      expect(events.api.getSnapshot).toHaveBeenCalledTimes(2);
      expect(events.api.getHistory).toHaveBeenCalledTimes(2);
      expect(store.session('project-1', 'D:/repo')).toMatchObject({
        activeTab: 'history',
        historyPage: 4,
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it('refreshes only the worktree containing a changed workspace file', async () => {
    vi.useFakeTimers();
    try {
      const events = eventApi();
      const store = new SourceControlStore(events.api);
      await store.ensureLoaded('project-1', 'D:/repo/worktree-a');

      events.emitWorkspace('D:/repo/worktree-b/src/other.ts');
      await vi.advanceTimersByTimeAsync(151);
      expect(events.api.getSnapshot).toHaveBeenCalledTimes(1);

      events.emitWorkspace('D:/repo/worktree-a/src/current.ts');
      await vi.advanceTimersByTimeAsync(151);
      expect(events.api.getSnapshot).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });
});

function fakeApi() {
  return {
    getSnapshot: vi.fn(async (_projectId: string, workspacePath?: string | null) => repositorySnapshot(workspacePath ?? 'D:/repo')),
    getHistory: vi.fn(async () => historyPage('history-1')),
    getCommitDetail: vi.fn(),
    analyzeCommitRelations: vi.fn(),
    executeMutation: vi.fn(),
    startOperation: vi.fn(),
    getOperation: vi.fn(),
    cancelOperation: vi.fn(),
  };
}

function eventApi() {
  let operationListener: ((operation: GitOperationVm) => void) | null = null;
  let stateListener: ((event: import('@/types').GitStateChangedEventVm) => void) | null = null;
  let workspaceListener: ((event: import('@/types').WorkspaceFileChangedEventVm) => void) | null = null;
  const api = {
    ...fakeApi(),
    startMonitor: vi.fn().mockResolvedValue(undefined),
    stopMonitor: vi.fn().mockResolvedValue(undefined),
    subscribeOperationUpdates: vi.fn(async (listener: (operation: GitOperationVm) => void) => {
      operationListener = listener;
      return () => { operationListener = null; };
    }),
    subscribeStateChanges: vi.fn(async (listener: (event: import('@/types').GitStateChangedEventVm) => void) => {
      stateListener = listener;
      return () => { stateListener = null; };
    }),
    subscribeWorkspaceChanges: vi.fn(async (listener: (event: import('@/types').WorkspaceFileChangedEventVm) => void) => {
      workspaceListener = listener;
      return () => { workspaceListener = null; };
    }),
  };
  return {
    api,
    emitOperation(operation: GitOperationVm) {
      operationListener?.(operation);
    },
    emitState(event: import('@/types').GitStateChangedEventVm) {
      stateListener?.(event);
    },
    emitWorkspace(canonicalPath: string) {
      workspaceListener?.({
        projectId: 'project-1',
        canonicalPath,
        kind: 'modified',
        revision: null,
        operationId: null,
      });
    },
  };
}

function repositorySnapshot(workspacePath: string, revision = 'revision-1'): GitSourceControlSnapshotVm {
  return {
    repository: {
      projectId: 'project-1',
      repoRoot: 'D:/repo',
      commonDir: 'D:/repo/.git',
      workspacePath,
      headOid: 'a'.repeat(40),
      currentBranch: 'main',
      detached: false,
      unborn: false,
      upstream: null,
      remotes: [],
      lock: { locked: false, owner: null, operation: null },
      revision,
    },
    status: {
      snapshotRevision: revision,
      branch: { oid: 'a'.repeat(40), head: 'main', upstream: null, ahead: 0, behind: 0 },
      conflicts: [],
      staged: [],
      unstaged: [],
      untracked: [],
      operationInProgress: null,
    },
    refs: [],
    worktrees: [],
    stashes: [],
  };
}

function historyPage(revision: string): GitHistoryPageVm {
  return { commits: [], nextCursor: null, revision };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}
