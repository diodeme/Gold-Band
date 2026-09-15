import { describe, expect, it, vi } from 'vitest';
import { SourceControlStore } from '@/components/workspace/source-control/source-control-store';
import i18n from '@/i18n';
import type {
  GitCapabilityVm,
  GitHistoryPageVm,
  GitCommitReviewVm,
  GitMutationResultVm,
  GitOperationVm,
  GitSourceControlSnapshotVm,
} from '@/types';

describe('source control session store', () => {
  it('models missing Git and non-repositories before requesting heavy repository data', async () => {
    const api = fakeApi();
    api.getCapability.mockResolvedValueOnce(capability('not-installed'));
    const store = new SourceControlStore(api);

    await store.ensureLoaded('project-1', 'D:/not-a-repository');

    expect(store.session('project-1', 'D:/not-a-repository')).toMatchObject({
      status: 'unavailable',
      capability: { status: 'not-installed' },
      error: null,
    });
    expect(api.getSnapshot).not.toHaveBeenCalled();
    expect(api.getHistory).not.toHaveBeenCalled();
  });

  it('stops at the unsupported Git version capability without repository data requests', async () => {
    const api = fakeApi();
    api.getCapability.mockResolvedValueOnce(capability('version-unsupported', '2.35.9'));
    const store = new SourceControlStore(api);

    await store.ensureLoaded('project-1', 'D:/repo');

    expect(store.session('project-1', 'D:/repo')).toMatchObject({
      status: 'unavailable',
      capability: {
        status: 'version-unsupported',
        installedVersion: '2.35.9',
        minimumVersion: '2.36.0',
      },
      error: null,
    });
    expect(api.getSnapshot).not.toHaveBeenCalled();
    expect(api.getHistory).not.toHaveBeenCalled();
  });

  it('initializes a non-repository and loads its unborn Git workspace without a second error state', async () => {
    const api = fakeApi();
    api.getCapability
      .mockResolvedValueOnce(capability('repository-required'))
      .mockResolvedValueOnce(capability('head-required'));
    api.initializeRepository.mockResolvedValueOnce(capability('head-required'));
    const emptySnapshot = repositorySnapshot('D:/repo');
    emptySnapshot.repository.headOid = null;
    emptySnapshot.repository.currentBranch = null;
    emptySnapshot.repository.unborn = true;
    api.getSnapshot.mockResolvedValueOnce(emptySnapshot);
    api.getHistory.mockResolvedValueOnce(historyPage('unborn-history'));
    const store = new SourceControlStore(api);
    await store.ensureLoaded('project-1', 'D:/repo');

    const request = store.initializeRepository('project-1', 'D:/repo');
    expect(store.session('project-1', 'D:/repo').pendingAction).toEqual({ kind: 'repository-initialize', path: null });
    await request;

    expect(api.initializeRepository).toHaveBeenCalledWith('project-1');
    expect(store.session('project-1', 'D:/repo')).toMatchObject({
      status: 'ready',
      capability: { status: 'head-required' },
      snapshot: { repository: { unborn: true } },
      error: null,
    });
  });

  it('localizes the aggregated changed-file count in both supported languages', () => {
    expect(i18n.t('sourceControl.changedFileCount', { count: 2, lng: 'zh-CN' })).toBe('2 个变更文件');
    expect(i18n.t('sourceControl.changedFileCount', { count: 2, lng: 'en' })).toBe('2 changed files');
  });

  it('localizes commit review topology and patch identity failures', () => {
    expect(i18n.t('errors.git.commit-review-topology-query-failed', { lng: 'zh-CN' })).not.toContain('errors.git');
    expect(i18n.t('errors.git.commit-review-patch-identity-failed', { lng: 'en' })).not.toContain('errors.git');
  });

  it('labels history pagination without claiming an unknown total page count', () => {
    expect(i18n.t('sourceControl.historyCurrentPage', { page: 3, lng: 'zh-CN' })).toBe('第 3 页');
    expect(i18n.t('sourceControl.historyCurrentPage', { page: 3, lng: 'en' })).toBe('Page 3');
  });

  it('restores repository data and view state after a Diff tab round trip without reloading', async () => {
    const api = fakeApi();
    const store = new SourceControlStore(api);

    await store.ensureLoaded('project-1', 'D:/repo');
    store.setActiveTab('project-1', 'D:/repo', 'history');
    store.setRepositoryTab('project-1', 'D:/repo', 'stashes');
    store.setHistoryPage('project-1', 'D:/repo', 2);
    store.setHistoryScrollPosition('project-1', 'D:/repo', 'commit-list', 144);
    store.setHistoryScrollPosition('project-1', 'D:/repo', 'review-list', 288, 'review-1');
    store.selectCommit('project-1', 'D:/repo', 'commit-1', ['commit-1'], { additive: false, range: false });

    // Remounting SourceControlWorkspacePanel calls ensureLoaded again.
    await store.ensureLoaded('project-1', 'D:/repo');

    expect(api.getSnapshot).toHaveBeenCalledTimes(1);
    expect(api.getHistory).toHaveBeenCalledTimes(1);
    expect(store.session('project-1', 'D:/repo')).toMatchObject({
      status: 'ready',
      activeTab: 'history',
      repositoryTab: 'stashes',
      historyPage: 2,
      canonicalWorkspacePath: 'D:/repo',
    });
    expect(store.session('project-1', 'D:/repo').selectedCommitOids.has('commit-1')).toBe(true);
    expect(store.historyScrollPositions('project-1', 'D:/repo', 'review-1')).toEqual({
      commitList: 144,
      reviewList: 288,
    });
    expect(store.historyScrollPositions('project-1', 'D:/repo', 'review-2').reviewList).toBe(0);
  });

  it('resets only the commit-list scroll position when changing history pages', async () => {
    const store = new SourceControlStore(fakeApi());
    await store.ensureLoaded('project-1', 'D:/repo');
    store.setHistoryScrollPosition('project-1', 'D:/repo', 'commit-list', 720);
    store.setHistoryScrollPosition('project-1', 'D:/repo', 'review-list', 240, 'review-1');

    store.setHistoryPage('project-1', 'D:/repo', 1);

    expect(store.historyScrollPositions('project-1', 'D:/repo', 'review-1')).toEqual({
      commitList: 0,
      reviewList: 240,
    });
  });

  it('reloads only on explicit refresh and resets stale history navigation', async () => {
    const api = fakeApi();
    const store = new SourceControlStore(api);
    await store.ensureLoaded('project-1', 'D:/repo');
    store.setHistoryPage('project-1', 'D:/repo', 3);
    store.selectCommit('project-1', 'D:/repo', 'commit-1', ['commit-1'], { additive: false, range: false });

    await store.refresh('project-1', 'D:/repo');

    expect(api.getSnapshot).toHaveBeenCalledTimes(2);
    expect(api.getHistory).toHaveBeenCalledTimes(2);
    expect(store.session('project-1', 'D:/repo')).toMatchObject({ historyPage: 0 });
    expect(store.session('project-1', 'D:/repo').selectedCommitOids.size).toBe(0);
  });

  it('deduplicates an in-flight next-page request and advances the cached history atomically', async () => {
    const nextPage = deferred<GitHistoryPageVm>();
    const api = fakeApi();
    api.getHistory
      .mockResolvedValueOnce({
        commits: Array.from({ length: 300 }, (_, index) => historyCommit(index)),
        nextCursor: 'cursor-300',
        revision: 'history-1',
      })
      .mockReturnValueOnce(nextPage.promise);
    const store = new SourceControlStore(api);
    await store.ensureLoaded('project-1', 'D:/repo');

    const firstRequest = store.loadMoreHistory('project-1', 'D:/repo', true);
    const duplicateRequest = store.loadMoreHistory('project-1', 'D:/repo', true);

    expect(api.getHistory).toHaveBeenCalledTimes(2);
    expect(api.getHistory).toHaveBeenLastCalledWith('project-1', 'D:/repo', {
      cursor: 'cursor-300',
      limit: 300,
      revision: 'history-1',
    });
    expect(store.session('project-1', 'D:/repo').pendingAction).toEqual({
      kind: 'history-more',
      path: null,
    });

    nextPage.resolve({
      commits: [historyCommit(300)],
      nextCursor: null,
      revision: 'history-1',
    });
    await Promise.all([firstRequest, duplicateRequest]);

    expect(store.session('project-1', 'D:/repo')).toMatchObject({
      historyPage: 1,
      pendingAction: null,
    });
    expect(store.session('project-1', 'D:/repo').history?.commits).toHaveLength(301);
  });

  it('merges a workspace-only mutation result without refreshing snapshot or history', async () => {
    const api = fakeApi();
    const store = new SourceControlStore(api);
    await store.ensureLoaded('project-1', 'D:/repo');
    const nextSnapshot = repositorySnapshot('D:/repo', 'revision-2');
    nextSnapshot.status.staged = [{
      path: 'src/app.ts',
      oldPath: null,
      kind: 'modified',
      indexStatus: 'M',
      worktreeStatus: null,
      binary: false,
      submodule: false,
      addedLines: 1,
      deletedLines: 0,
    }];
    api.executeMutation.mockResolvedValueOnce({
      scope: 'workspace',
      status: nextSnapshot.status,
      repositoryRevision: 'revision-2',
    });

    await store.mutate('project-1', 'D:/repo', { kind: 'stage-all' });

    expect(api.executeMutation).toHaveBeenCalledWith('project-1', 'D:/repo', {
      kind: 'stage-all',
      expectedRevision: 'revision-1',
    });
    expect(api.getSnapshot).toHaveBeenCalledTimes(1);
    expect(api.getHistory).toHaveBeenCalledTimes(1);
    expect(store.session('project-1', 'D:/repo').snapshot?.repository.revision).toBe('revision-2');
    expect(store.session('project-1', 'D:/repo').snapshot?.status.staged[0]?.path).toBe('src/app.ts');
  });

  it('implements single, additive, range, and additive-range commit selection', async () => {
    const api = fakeApi();
    const store = new SourceControlStore(api);
    await store.ensureLoaded('project-1', 'D:/repo');
    const visible = ['commit-1', 'commit-2', 'commit-3', 'commit-4'];

    store.selectCommit('project-1', 'D:/repo', 'commit-2', visible, { additive: false, range: false });
    store.selectCommit('project-1', 'D:/repo', 'commit-4', visible, { additive: false, range: true });
    expect([...store.session('project-1', 'D:/repo').selectedCommitOids]).toEqual(['commit-2', 'commit-3', 'commit-4']);

    store.selectCommit('project-1', 'D:/repo', 'commit-1', visible, { additive: true, range: false });
    expect([...store.session('project-1', 'D:/repo').selectedCommitOids]).toEqual(['commit-1', 'commit-2', 'commit-3', 'commit-4']);

    store.selectCommit('project-1', 'D:/repo', 'commit-3', visible, { additive: true, range: true });
    expect(new Set(store.session('project-1', 'D:/repo').selectedCommitOids)).toEqual(new Set(visible));
  });

  it('preserves a selected group on right-click and selects an unselected commit alone', async () => {
    const api = fakeApi();
    const store = new SourceControlStore(api);
    await store.ensureLoaded('project-1', 'D:/repo');
    const visible = ['commit-1', 'commit-2', 'commit-3'];
    store.selectCommit('project-1', 'D:/repo', 'commit-1', visible, { additive: false, range: false });
    store.selectCommit('project-1', 'D:/repo', 'commit-2', visible, { additive: true, range: false });

    store.selectCommitForContextMenu('project-1', 'D:/repo', 'commit-2');
    expect([...store.session('project-1', 'D:/repo').selectedCommitOids]).toEqual(['commit-1', 'commit-2']);

    store.selectCommitForContextMenu('project-1', 'D:/repo', 'commit-3');
    expect([...store.session('project-1', 'D:/repo').selectedCommitOids]).toEqual(['commit-3']);
  });

  it('prevents an older commit review response from replacing the latest selection', async () => {
    const first = deferred<GitCommitReviewVm>();
    const second = deferred<GitCommitReviewVm>();
    const api = fakeApi();
    api.getCommitReview.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const store = new SourceControlStore(api);
    await store.ensureLoaded('project-1', 'D:/repo');
    const visible = ['commit-1', 'commit-2'];
    store.selectCommit('project-1', 'D:/repo', 'commit-1', visible, { additive: false, range: false });
    store.selectCommit('project-1', 'D:/repo', 'commit-2', visible, { additive: false, range: false });

    second.resolve(commitReview(['commit-2']));
    await vi.waitFor(() => expect(store.session('project-1', 'D:/repo').commitReview?.selectedOids).toEqual(['commit-2']));
    first.resolve(commitReview(['commit-1']));
    await Promise.resolve();

    expect(store.session('project-1', 'D:/repo').commitReview?.selectedOids).toEqual(['commit-2']);
  });

  it('publishes commit selection and its loading state before review I/O completes', async () => {
    const review = deferred<GitCommitReviewVm>();
    const api = fakeApi();
    api.getCommitReview.mockReturnValueOnce(review.promise);
    const store = new SourceControlStore(api);
    await store.ensureLoaded('project-1', 'D:/repo');

    store.selectCommit('project-1', 'D:/repo', 'commit-1', ['commit-1'], {
      additive: false,
      range: false,
    });

    expect(store.session('project-1', 'D:/repo')).toMatchObject({
      focusedCommitOid: 'commit-1',
      historyDetailLoading: true,
      commitReview: null,
    });
    expect(store.session('project-1', 'D:/repo').selectedCommitOids.has('commit-1')).toBe(true);

    review.resolve(commitReview(['commit-1']));
    await vi.waitFor(() => expect(store.session('project-1', 'D:/repo').historyDetailLoading).toBe(false));
  });

  it('reuses a commit review result for the same ordered selection and revision', async () => {
    const api = fakeApi();
    const store = new SourceControlStore(api);
    await store.ensureLoaded('project-1', 'D:/repo');
    const visible = ['commit-1', 'commit-2'];

    store.selectCommit('project-1', 'D:/repo', 'commit-1', visible, { additive: false, range: false });
    await vi.waitFor(() => expect(store.session('project-1', 'D:/repo').historyDetailLoading).toBe(false));
    store.clearCommitSelection('project-1', 'D:/repo');
    store.selectCommit('project-1', 'D:/repo', 'commit-1', visible, { additive: false, range: false });
    await vi.waitFor(() => expect(store.session('project-1', 'D:/repo').historyDetailLoading).toBe(false));

    expect(api.getCommitReview).toHaveBeenCalledTimes(1);
  });

  it('drops commit review cache entries when their repository session is cleared', async () => {
    const api = fakeApi();
    const store = new SourceControlStore(api);
    const visible = ['commit-1'];
    await store.ensureLoaded('project-1', 'D:/repo');
    store.selectCommit('project-1', 'D:/repo', 'commit-1', visible, { additive: false, range: false });
    await vi.waitFor(() => expect(store.session('project-1', 'D:/repo').historyDetailLoading).toBe(false));

    store.clear('project-1', 'D:/repo');
    await store.ensureLoaded('project-1', 'D:/repo');
    store.selectCommit('project-1', 'D:/repo', 'commit-1', visible, { additive: false, range: false });
    await vi.waitFor(() => expect(store.session('project-1', 'D:/repo').historyDetailLoading).toBe(false));

    expect(api.getCommitReview).toHaveBeenCalledTimes(2);
  });

  it('tracks the pending file identity and rejects a second mutation until Stage settles', async () => {
    const result = deferred<GitMutationResultVm>();
    const api = fakeApi();
    api.executeMutation.mockReturnValueOnce(result.promise);
    const store = new SourceControlStore(api);
    await store.ensureLoaded('project-1', 'D:/repo');

    const mutation = store.mutate('project-1', 'D:/repo', { kind: 'stage-paths', paths: ['src/app.ts'] });
    expect(store.session('project-1', 'D:/repo').pendingAction).toEqual({
      kind: 'stage-paths',
      path: 'src/app.ts',
    });
    await store.mutate('project-1', 'D:/repo', { kind: 'stage-paths', paths: ['src/other.ts'] });
    expect(api.executeMutation).toHaveBeenCalledTimes(1);

    const nextSnapshot = repositorySnapshot('D:/repo', 'revision-2');
    result.resolve({
      scope: 'workspace',
      status: nextSnapshot.status,
      repositoryRevision: 'revision-2',
    });
    await mutation;
    expect(store.session('project-1', 'D:/repo').pendingAction).toBeNull();
  });

  it('refreshes repository snapshot and history in parallel after a ref-changing mutation', async () => {
    const nextSnapshot = deferred<GitSourceControlSnapshotVm>();
    const nextHistory = deferred<GitHistoryPageVm>();
    const api = fakeApi();
    const store = new SourceControlStore(api);
    await store.ensureLoaded('project-1', 'D:/repo');
    api.executeMutation.mockResolvedValueOnce({ scope: 'repository' });
    api.getSnapshot.mockReturnValueOnce(nextSnapshot.promise);
    api.getHistory.mockReturnValueOnce(nextHistory.promise);

    const mutation = store.mutate('project-1', 'D:/repo', { kind: 'commit', subject: 'Commit', body: null });
    await vi.waitFor(() => {
      expect(api.getSnapshot).toHaveBeenCalledTimes(2);
      expect(api.getHistory).toHaveBeenCalledTimes(2);
    });

    nextSnapshot.resolve(repositorySnapshot('D:/repo', 'revision-2'));
    nextHistory.resolve(historyPage('revision-2'));
    await mutation;
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
    store.setRepositoryTab('project-1', 'd:/REPO/worktree-a', 'worktrees');
    store.setSubject('project-1', 'd:/REPO/worktree-a', 'Worktree A draft');
    store.setActiveTab('project-1', 'D:/repo/worktree-b', 'history');

    expect(store.session('project-1', 'D:/repo/worktree-a')).toMatchObject({
      activeTab: 'github',
      repositoryTab: 'worktrees',
      subject: 'Worktree A draft',
    });
    expect(store.session('project-1', 'D:/repo/worktree-b')).toMatchObject({
      activeTab: 'history',
      repositoryTab: 'branches',
      subject: '',
    });
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
    await vi.waitFor(() => expect(api.getSnapshot).toHaveBeenCalledTimes(1));
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

    await store.startOperation('project-1', 'D:/repo', { kind: 'fetch', prune: true });
    events.emitOperation({
      ...queued,
      status: 'succeeded',
      cancelable: false,
      completedAt: '2026-08-11T00:00:00.000Z',
    });

    await vi.waitFor(() => expect(store.session('project-1', 'D:/repo').pendingAction).toBeNull());
    expect(events.api.getSnapshot).toHaveBeenCalledTimes(2);
    expect(events.api.getOperation).not.toHaveBeenCalled();
    expect(store.session('project-1', 'D:/repo').activeOperation).toMatchObject({
      operationId: 'operation-1',
      status: 'succeeded',
    });

    store.setActiveTab('project-1', 'D:/repo', 'repository');
    store.setActiveTab('project-1', 'D:/repo', 'changes');
    expect(store.session('project-1', 'D:/repo').activeOperation?.status).toBe('succeeded');

    store.dismissOperationResult('project-1', 'D:/repo');
    expect(store.session('project-1', 'D:/repo').activeOperation).toBeNull();
  });

  it('preserves a structured Git failure reason after the terminal refresh', async () => {
    const events = eventApi();
    const queued: GitOperationVm = {
      operationId: 'operation-failed',
      kind: 'push',
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

    await store.startOperation('project-1', 'D:/repo', { kind: 'push', remote: 'origin', branch: 'main', setUpstream: true });
    events.emitOperation({
      ...queued,
      status: 'failed',
      cancelable: false,
      completedAt: '2026-08-11T00:00:00.000Z',
      error: {
        code: 'git.authentication-failed',
        params: { exitCode: 128, reason: "fatal: Authentication failed for 'https://github.com/example/repo.git/'" },
      },
    });

    await vi.waitFor(() => expect(store.session('project-1', 'D:/repo').pendingAction).toBeNull());
    expect(store.session('project-1', 'D:/repo').error).toEqual({
      code: 'git.authentication-failed',
      params: { exitCode: 128, reason: "fatal: Authentication failed for 'https://github.com/example/repo.git/'" },
    });

    store.dismissOperationResult('project-1', 'D:/repo');
    expect(store.session('project-1', 'D:/repo').activeOperation).toBeNull();
    expect(store.session('project-1', 'D:/repo').error).toBeNull();
  });

  it('debounces matching repository events and preserves navigation during snapshot invalidation', async () => {
    vi.useFakeTimers();
    try {
      const events = eventApi();
      const store = new SourceControlStore(events.api);
      await store.ensureLoaded('project-1', 'D:/repo');
      store.setActiveTab('project-1', 'D:/repo', 'history');
      store.setHistoryPage('project-1', 'D:/repo', 4);
      store.setSubject('project-1', 'D:/repo', 'draft subject');
      store.setBody('project-1', 'D:/repo', 'draft body');
      const snapshotRefresh = deferred<GitSourceControlSnapshotVm>();
      const historyRefresh = deferred<GitHistoryPageVm>();
      events.api.getSnapshot.mockReturnValueOnce(snapshotRefresh.promise);
      events.api.getHistory.mockReturnValueOnce(historyRefresh.promise);

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
        pendingAction: null,
        refreshing: 'background',
        subject: 'draft subject',
        body: 'draft body',
      });
      snapshotRefresh.resolve(repositorySnapshot('D:/repo', 'revision-2'));
      historyRefresh.resolve(historyPage('history-2'));
      await vi.advanceTimersByTimeAsync(0);
      expect(store.session('project-1', 'D:/repo').refreshing).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('establishes the repository monitor before taking the first authoritative snapshot', async () => {
    const events = eventApi();
    const monitorStarted = deferred<void>();
    events.api.startMonitor.mockReturnValueOnce(monitorStarted.promise);
    const store = new SourceControlStore(events.api);

    const loading = store.ensureLoaded('project-1', 'D:/repo');
    await vi.waitFor(() => expect(events.api.startMonitor).toHaveBeenCalledTimes(1));
    expect(events.api.getSnapshot).not.toHaveBeenCalled();

    monitorStarted.resolve();
    await loading;
    expect(events.api.getSnapshot).toHaveBeenCalledTimes(1);
  });

  it('treats a null workspace path as the main repository monitor scope', async () => {
    const events = eventApi();
    const monitorStarted = deferred<void>();
    events.api.startMonitor.mockReturnValueOnce(monitorStarted.promise);
    const store = new SourceControlStore(events.api);

    const loading = store.ensureLoaded('project-1', null);
    await vi.waitFor(() => expect(events.api.startMonitor).toHaveBeenCalledWith('project-1', null));
    expect(events.api.getSnapshot).not.toHaveBeenCalled();

    monitorStarted.resolve();
    await loading;
    expect(events.api.getSnapshot).toHaveBeenCalledTimes(1);
  });

  it('refreshes worktree status without reloading history for ordinary workspace changes', async () => {
    vi.useFakeTimers();
    try {
      const events = eventApi();
      const store = new SourceControlStore(events.api);
      await store.ensureLoaded('project-1', 'D:/repo');

      events.emitWorkspace('D:/repo/src/current.ts');
      await vi.advanceTimersByTimeAsync(151);

      expect(events.api.getSnapshot).toHaveBeenCalledTimes(2);
      expect(events.api.getHistory).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it('bounds workspace-event debounce latency under continuous writes', async () => {
    vi.useFakeTimers();
    try {
      const events = eventApi();
      const store = new SourceControlStore(events.api);
      await store.ensureLoaded('project-1', 'D:/repo');

      for (let elapsed = 0; elapsed < 1_000; elapsed += 100) {
        events.emitWorkspace('D:/repo/src/generated.ts');
        await vi.advanceTimersByTimeAsync(100);
      }

      expect(events.api.getSnapshot).toHaveBeenCalledTimes(2);
      expect(events.api.getHistory).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it('retains an invalidation received while a workspace mutation is pending', async () => {
    vi.useFakeTimers();
    try {
      const events = eventApi();
      const mutationResult = deferred<GitMutationResultVm>();
      events.api.executeMutation.mockReturnValueOnce(mutationResult.promise);
      const store = new SourceControlStore(events.api);
      await store.ensureLoaded('project-1', 'D:/repo');

      const mutation = store.mutate('project-1', 'D:/repo', { kind: 'stage-all' });
      events.emitWorkspace('D:/repo/src/current.ts');
      await vi.advanceTimersByTimeAsync(1_000);
      expect(events.api.getSnapshot).toHaveBeenCalledTimes(1);

      mutationResult.resolve({
        scope: 'workspace',
        status: repositorySnapshot('D:/repo', 'revision-2').status,
        repositoryRevision: 'revision-2',
      });
      await mutation;
      await vi.advanceTimersByTimeAsync(0);

      expect(events.api.getSnapshot).toHaveBeenCalledTimes(2);
      expect(events.api.getHistory).toHaveBeenCalledTimes(1);
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
    getCapability: vi.fn(async () => capability('ready')),
    initializeRepository: vi.fn(async () => capability('head-required')),
    getSnapshot: vi.fn(async (_projectId: string, workspacePath?: string | null) => repositorySnapshot(workspacePath ?? 'D:/repo')),
    getHistory: vi.fn(async () => historyPage('history-1')),
    getCommitReview: vi.fn(async (_projectId: string, _workspacePath: string | null | undefined, query: { selectedOids: string[] }) => ({
      selectedOids: query.selectedOids,
      revision: 'history-1',
      files: [],
      totals: { commitCount: query.selectedOids.length, fileCount: 0 },
    })),
    getCommitReachability: vi.fn(),
    executeMutation: vi.fn(),
    startOperation: vi.fn(),
    getOperation: vi.fn(),
    cancelOperation: vi.fn(),
  };
}

function capability(status: GitCapabilityVm['status'], installedVersion = '2.53.0'): GitCapabilityVm {
  return {
    status,
    installedVersion: status === 'not-installed' || status === 'version-unavailable' ? null : installedVersion,
    minimumVersion: '2.36.0',
    repoRoot: status === 'not-installed' || status === 'repository-required' ? null : 'D:/repo',
    commonDir: status === 'not-installed' || status === 'repository-required' ? null : 'D:/repo/.git',
    head: status === 'ready' ? 'a'.repeat(40) : null,
  };
}

function commitReview(selectedOids: string[]): GitCommitReviewVm {
  return {
    selectedOids,
    revision: 'history-1',
    files: [],
    totals: { commitCount: selectedOids.length, fileCount: 0 },
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

function historyCommit(index: number): GitHistoryPageVm['commits'][number] {
  const oid = index.toString(16).padStart(40, '0');
  return {
    oid,
    parentOids: index > 0 ? [(index - 1).toString(16).padStart(40, '0')] : [],
    subject: `commit ${index}`,
    body: '',
    author: { name: 'Ada', email: null, timestamp: '2026-08-12T00:00:00Z' },
    committer: { name: 'Ada', email: null, timestamp: '2026-08-12T00:00:00Z' },
    refs: [],
    sourceRef: 'refs/heads/main',
    runtimeCheckpoint: false,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}
