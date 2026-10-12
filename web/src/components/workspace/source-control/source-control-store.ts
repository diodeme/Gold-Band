import { createSourceControlLoadDiagnostic } from '@/lib/source-control-load-diagnostics';
import type { SourceControlWatchReport } from '@/lib/source-control-watch-diagnostics';
import { useCallback, useSyncExternalStore } from 'react';
import {
  reportSourceControlLoad,
  reportSourceControlWatch,
  cancelGitOperation,
  executeGitMutation,
  getGitCapability,
  getGitCommitReachability,
  getGitCommitReview,
  getGitCommitReviewStatistics,
  getGitHistory,
  getSourceControlBootstrap,
  getSourceControlSnapshot,
  getSourceControlOverview,
  getSourceControlRemotes,
  getSourceControlStatistics,
  initializeGitRepository,
  startGitOperation,
  startGitStateMonitor,
  stopGitStateMonitor,
  subscribeGitOperationUpdates,
  subscribeGitStateChanges,
  subscribeWorkspaceFileChanges,
} from '@/api';
import type {
  GitCommitReachabilityVm,
  GitCommitReviewVm,
  GitCapabilityVm,
  GitHistoryPageVm,
  GitMutationRequestVm,
  GitOperationRequestVm,
  GitOperationErrorVm,
  GitOperationVm,
  GitStateChangedEventVm,
  GitSourceControlSnapshotVm,
  GitSourceControlOverviewVm,
  WorkspaceFileChangedEventVm,
} from '@/types';
import { diffReviewStore } from './diff-review-store';
import {
  normalizeWorkspacePath,
  workspaceRootKey,
} from '@/lib/workspace-root';

export type SourceControlTab = 'changes' | 'history' | 'repository' | 'github';
export type SourceControlRepositoryTab = 'branches' | 'tags' | 'worktrees' | 'stashes';
export interface CommitSelectionModifiers {
  additive: boolean;
  range: boolean;
}
export interface SourceControlPendingAction {
  kind: GitMutationRequestVm['kind'] | GitOperationRequestVm['kind'] | 'history-more' | 'repository-initialize';
  path: string | null;
}

export type SourceControlRefreshKind = 'manual' | 'background';

export interface SourceControlSessionSnapshot {
  projectId: string;
  requestedWorkspacePath: string | null;
  canonicalWorkspacePath: string | null;
  status: 'idle' | 'loading' | 'ready' | 'unavailable' | 'error';
  capability: GitCapabilityVm | null;
  snapshot: import('@/types').GitSourceControlOverviewVm | null;
  catalog: GitSourceControlSnapshotVm | null;
  catalogLoading: boolean;
  catalogError: GitOperationErrorVm | null;
  remotesLoading: boolean;
  remotesError: GitOperationErrorVm | null;
  history: GitHistoryPageVm | null;
  historyLoading: boolean;
  activeTab: SourceControlTab;
  repositoryTab: SourceControlRepositoryTab;
  historyPage: number;
  selectedCommitOids: ReadonlySet<string>;
  selectionAnchorOid: string | null;
  focusedCommitOid: string | null;
  commitReview: GitCommitReviewVm | null;
  commitReachability: GitCommitReachabilityVm | null;
  historyDetailLoading: boolean;
  reviewStatisticsLoading?: boolean;
  reviewStatisticsError?: GitOperationErrorVm | null;
  reachabilityLoading: boolean;
  error: GitOperationErrorVm | null;
  pendingAction: SourceControlPendingAction | null;
  refreshing: SourceControlRefreshKind | null;
  activeOperation: GitOperationVm | null;
  subject: string;
  body: string;
}

interface SourceControlApi {
  reportLoad?: typeof reportSourceControlLoad;
  reportWatch?: typeof reportSourceControlWatch;
  getCapability: typeof getGitCapability;
  getBootstrap?: typeof getSourceControlBootstrap;
  initializeRepository: typeof initializeGitRepository;
  getSnapshot: typeof getSourceControlOverview;
  getCatalog: typeof getSourceControlSnapshot;
  getStatistics?: typeof getSourceControlStatistics;
  getRemotes?: typeof getSourceControlRemotes;
  getHistory: typeof getGitHistory;
  getCommitReview: typeof getGitCommitReview;
  getCommitReviewStatistics?: typeof getGitCommitReviewStatistics;
  getCommitReachability: typeof getGitCommitReachability;
  executeMutation: typeof executeGitMutation;
  startOperation: typeof startGitOperation;
  cancelOperation: typeof cancelGitOperation;
  startMonitor?: typeof startGitStateMonitor;
  stopMonitor?: typeof stopGitStateMonitor;
  subscribeOperationUpdates?: typeof subscribeGitOperationUpdates;
  subscribeStateChanges?: typeof subscribeGitStateChanges;
  subscribeWorkspaceChanges?: typeof subscribeWorkspaceFileChanges;
}

interface SessionRuntime {
  storageKey: string;
  snapshot: SourceControlSessionSnapshot;
  listeners: Set<() => void>;
  repositoryRequestRevision: number;
  historyRequestRevision: number;
  historyPromise: Promise<void> | null;
  detailRequestRevision: number;
  reachabilityRequestRevision: number;
  loadPromise: Promise<void> | null;
  catalogPromise: Promise<void> | null;
  catalogRequestRevision: number;
  catalogLoadedRevision: number;
  statisticsPromise: Promise<void> | null;
  remotesPromise: Promise<void> | null;
  monitorStarted: boolean;
  monitorStartPromise: Promise<void> | null;
  invalidationTimer: ReturnType<typeof setTimeout> | null;
  invalidationStartedAt: number | null;
  pendingInvalidation: SourceControlInvalidationScope | null;
  pendingDiffPaths: Set<string>;
  pendingDiffAll: boolean;
  finishingOperationId: string | null;
  historyCommitScrollTop: number;
  historyReviewScrollTop: number;
  historyReviewScrollKey: string | null;
}

interface CommitReviewCacheSlot {
  statistics?: Promise<GitCommitReviewVm>;
  statisticsReady?: boolean;
  value?: GitCommitReviewVm;
  request?: Promise<GitCommitReviewVm>;
}

interface WorkspaceEventDiagnosticBatch {
  projectId: string;
  receivedEvents: number;
  projectSessions: number;
  routedSessions: number;
  metadataFilteredSessions: number;
  outOfScopeSessions: number;
  scopeMismatchSessions: number;
  nestedWorktreeFilteredSessions: number;
  pathOutsideWorkspaceSessions: number;
}

const DEFAULT_API: SourceControlApi = {
  reportLoad: reportSourceControlLoad,
  reportWatch: reportSourceControlWatch,
  getCapability: getGitCapability,
  getBootstrap: getSourceControlBootstrap,
  initializeRepository: initializeGitRepository,
  getSnapshot: getSourceControlOverview,
  getCatalog: getSourceControlSnapshot,
  getStatistics: getSourceControlStatistics,
  getRemotes: getSourceControlRemotes,
  getHistory: getGitHistory,
  getCommitReview: getGitCommitReview,
  getCommitReviewStatistics: getGitCommitReviewStatistics,
  getCommitReachability: getGitCommitReachability,
  executeMutation: executeGitMutation,
  startOperation: startGitOperation,
  cancelOperation: cancelGitOperation,
  startMonitor: startGitStateMonitor,
  stopMonitor: stopGitStateMonitor,
  subscribeOperationUpdates: subscribeGitOperationUpdates,
  subscribeStateChanges: subscribeGitStateChanges,
  subscribeWorkspaceChanges: subscribeWorkspaceFileChanges,
};

const HISTORY_PAGE_SIZE = 300;
const STATE_INVALIDATION_DEBOUNCE_MS = 150;
const STATE_INVALIDATION_MAX_LATENCY_MS = 1_000;
const WATCH_DIAGNOSTIC_FLUSH_MS = 250;
type SourceControlInvalidationScope = 'worktree' | 'repository';

export class SourceControlStore {
  static readonly MAX_SESSIONS = 24;

  private readonly sessions = new Map<string, SessionRuntime>();
  private readonly aliases = new Map<string, string>();
  private readonly earlyOperationUpdates = new Map<string, GitOperationVm>();
  private readonly commitReviews = new Map<string, CommitReviewCacheSlot>();
  private readonly workspaceEventDiagnostics = new Map<string, WorkspaceEventDiagnosticBatch>();
  private subscriptionsPromise: Promise<void> | null = null;
  private workspaceEventDiagnosticTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(private readonly api: SourceControlApi = DEFAULT_API) {
    void this.ensureSubscriptions();
  }

  subscribe(projectId: string, workspacePath: string | null | undefined, listener: () => void) {
    const runtime = this.runtime(projectId, workspacePath);
    runtime.listeners.add(listener);
    return () => runtime.listeners.delete(listener);
  }

  session(projectId: string, workspacePath?: string | null) {
    return this.runtime(projectId, workspacePath, false).snapshot;
  }

  ensureLoaded(projectId: string, workspacePath?: string | null) {
    return this.load(projectId, workspacePath, false);
  }

  refresh(projectId: string, workspacePath?: string | null) {
    return this.load(projectId, workspacePath, true, true, 'manual');
  }

  retryRemotes(projectId: string, workspacePath?: string | null) {
    return this.loadRemotes(this.runtime(projectId, workspacePath));
  }

  async initializeRepository(projectId: string, workspacePath?: string | null) {
    const runtime = this.runtime(projectId, workspacePath);
    if (runtime.snapshot.pendingAction || runtime.snapshot.capability?.status !== 'repository-required') return;
    this.update(runtime, {
      ...runtime.snapshot,
      pendingAction: { kind: 'repository-initialize', path: null },
      error: null,
    });
    try {
      const capability = await this.api.initializeRepository(projectId);
      this.update(runtime, { ...runtime.snapshot, capability, pendingAction: null });
      if (capability.status === 'ready' || capability.status === 'head-required') {
        await this.load(projectId, workspacePath, true, true, 'manual');
      } else {
        this.update(runtime, { ...runtime.snapshot, status: 'unavailable' });
      }
    } catch (reason) {
      this.update(runtime, {
        ...runtime.snapshot,
        status: 'error',
        pendingAction: null,
        error: structuredErrorFrom(reason, 'git.repository-initialize-failed'),
      });
    }
  }

  setActiveTab(projectId: string, workspacePath: string | null | undefined, activeTab: SourceControlTab) {
    const runtime = this.runtime(projectId, workspacePath);
    if (activeTab === 'repository' || activeTab === 'github') {
      this.update(runtime, { ...runtime.snapshot, activeTab });
      return this.ensureCatalog(runtime);
    }
    if (runtime.snapshot.activeTab === activeTab) {
      return activeTab === 'history' ? this.ensureHistory(projectId, workspacePath) : Promise.resolve();
    }
    const historyLoading = activeTab === 'history' && runtime.snapshot.history == null;
    this.update(runtime, { ...runtime.snapshot, activeTab, historyLoading });
    return historyLoading ? this.ensureHistory(projectId, workspacePath) : Promise.resolve();
  }

  private ensureCatalog(runtime: SessionRuntime): Promise<void> {
    if (runtime.catalogPromise) return runtime.catalogPromise;
    if (!runtime.snapshot.snapshot || runtime.snapshot.refreshing
      || (runtime.snapshot.catalog && runtime.catalogLoadedRevision === runtime.catalogRequestRevision)) return Promise.resolve();
    const revision = runtime.catalogRequestRevision;
    this.update(runtime, { ...runtime.snapshot, catalogLoading: true, catalogError: null });
    const request = this.api.getCatalog(runtime.snapshot.projectId, runtime.snapshot.requestedWorkspacePath)
      .then((catalog) => {
        if (runtime.catalogRequestRevision !== revision || this.sessions.get(runtime.storageKey) !== runtime) return;
        runtime.catalogLoadedRevision = revision;
        const snapshot = runtime.snapshot.snapshot;
        this.update(runtime, {
          ...runtime.snapshot,
          snapshot: snapshot
            ? { ...snapshot, repository: { ...snapshot.repository, remotes: catalog.repository.remotes } }
            : snapshot,
          catalog,
          catalogLoading: false,
          remotesLoading: false,
          remotesError: null,
        });
      }).catch((reason) => {
        if (runtime.catalogRequestRevision !== revision || this.sessions.get(runtime.storageKey) !== runtime) return;
        this.update(runtime, { ...runtime.snapshot, catalogLoading: false, catalogError: structuredErrorFrom(reason, 'git.status-failed') });
      }).finally(() => {
        runtime.catalogPromise = null;
        if (runtime.catalogRequestRevision !== revision && this.sessions.get(runtime.storageKey) === runtime) this.loadVisibleDetails(runtime);
      });
    runtime.catalogPromise = request;
    return request;
  }

  private loadVisibleDetails(runtime: SessionRuntime) {
    if (runtime.snapshot.activeTab === 'repository' || runtime.snapshot.activeTab === 'github') void this.ensureCatalog(runtime);
  }

  private loadStatistics(runtime: SessionRuntime, loadId?: string) {
    if (!this.api.getStatistics || runtime.statisticsPromise || !runtime.snapshot.snapshot || runtime.pendingInvalidation) return;
    const revision = runtime.repositoryRequestRevision;
    const snapshot = runtime.snapshot.snapshot;
    const request = this.api.getStatistics(runtime.snapshot.projectId, runtime.snapshot.requestedWorkspacePath, loadId)
      .then((status) => {
        if (runtime.repositoryRequestRevision !== revision || this.sessions.get(runtime.storageKey) !== runtime) return;
        if (status.snapshotRevision !== snapshot.repository.revision) return;
        this.update(runtime, { ...runtime.snapshot, snapshot: { ...runtime.snapshot.snapshot!, status } });
        diffReviewStore.publishWorkspaceRefresh({
          projectId: runtime.snapshot.projectId,
          workspacePath: runtime.snapshot.requestedWorkspacePath,
          staged: status.staged,
          unstaged: status.unstaged,
          untracked: status.untracked,
          invalidate: { all: false, paths: [] },
        });
      }).catch(() => undefined).finally(() => {
        runtime.statisticsPromise = null;
        if (runtime.repositoryRequestRevision !== revision && this.sessions.get(runtime.storageKey) === runtime) this.loadStatistics(runtime);
      });
    runtime.statisticsPromise = request;
  }

  private loadRemotes(runtime: SessionRuntime, loadId?: string): Promise<void> {
    if (!this.api.getRemotes || !runtime.snapshot.snapshot) return Promise.resolve();
    if (runtime.remotesPromise) return runtime.remotesPromise;
    const revision = runtime.repositoryRequestRevision;
    if (!runtime.snapshot.remotesLoading || runtime.snapshot.remotesError) {
      this.update(runtime, { ...runtime.snapshot, remotesLoading: true, remotesError: null });
    }
    const request = this.api.getRemotes(
      runtime.snapshot.projectId,
      runtime.snapshot.requestedWorkspacePath,
      loadId,
    ).then((remotes) => {
      if (runtime.repositoryRequestRevision !== revision || this.sessions.get(runtime.storageKey) !== runtime) return;
      const snapshot = runtime.snapshot.snapshot;
      const catalog = runtime.snapshot.catalog;
      if (!snapshot) return;
      this.update(runtime, {
        ...runtime.snapshot,
        snapshot: { ...snapshot, repository: { ...snapshot.repository, remotes } },
        catalog: catalog
          ? { ...catalog, repository: { ...catalog.repository, remotes } }
          : catalog,
        remotesLoading: false,
        remotesError: null,
      });
    }).catch((reason) => {
      if (runtime.repositoryRequestRevision !== revision || this.sessions.get(runtime.storageKey) !== runtime) return;
      this.update(runtime, {
        ...runtime.snapshot,
        remotesLoading: false,
        remotesError: structuredErrorFrom(reason, 'git.status-failed'),
      });
    }).finally(() => {
      if (runtime.remotesPromise === request) runtime.remotesPromise = null;
      if (runtime.repositoryRequestRevision !== revision && this.sessions.get(runtime.storageKey) === runtime) {
        void this.loadRemotes(runtime);
      }
    });
    runtime.remotesPromise = request;
    return request;
  }

  setHistoryPage(projectId: string, workspacePath: string | null | undefined, historyPage: number) {
    const runtime = this.runtime(projectId, workspacePath);
    const next = Math.max(0, Math.floor(historyPage));
    if (runtime.snapshot.historyPage === next) return;
    runtime.historyCommitScrollTop = 0;
    this.update(runtime, { ...runtime.snapshot, historyPage: next });
  }

  historyScrollPositions(
    projectId: string,
    workspacePath: string | null | undefined,
    reviewKey?: string | null,
  ) {
    const runtime = this.runtime(projectId, workspacePath, false);
    return {
      commitList: runtime.historyCommitScrollTop,
      reviewList: reviewKey != null && runtime.historyReviewScrollKey === reviewKey
        ? runtime.historyReviewScrollTop
        : 0,
    };
  }

  setHistoryScrollPosition(
    projectId: string,
    workspacePath: string | null | undefined,
    area: 'commit-list' | 'review-list',
    scrollTop: number,
    reviewKey?: string | null,
  ) {
    const runtime = this.runtime(projectId, workspacePath, false);
    const next = Math.max(0, scrollTop);
    if (area === 'commit-list') runtime.historyCommitScrollTop = next;
    else {
      runtime.historyReviewScrollKey = reviewKey ?? null;
      runtime.historyReviewScrollTop = next;
    }
  }

  selectCommit(
    projectId: string,
    workspacePath: string | null | undefined,
    oid: string,
    visibleOids: readonly string[],
    modifiers: CommitSelectionModifiers,
  ) {
    const runtime = this.runtime(projectId, workspacePath);
    let selectedCommitOids: Set<string>;
    let selectionAnchorOid = runtime.snapshot.selectionAnchorOid;
    if (modifiers.range && selectionAnchorOid) {
      const anchorIndex = visibleOids.indexOf(selectionAnchorOid);
      const targetIndex = visibleOids.indexOf(oid);
      if (anchorIndex >= 0 && targetIndex >= 0) {
        selectedCommitOids = modifiers.additive
          ? new Set(runtime.snapshot.selectedCommitOids)
          : new Set<string>();
        const start = Math.min(anchorIndex, targetIndex);
        const end = Math.max(anchorIndex, targetIndex);
        for (let index = start; index <= end; index += 1) selectedCommitOids.add(visibleOids[index]);
      } else {
        selectedCommitOids = new Set([oid]);
        selectionAnchorOid = oid;
      }
    } else if (modifiers.additive) {
      selectedCommitOids = new Set(runtime.snapshot.selectedCommitOids);
      if (selectedCommitOids.has(oid)) selectedCommitOids.delete(oid);
      else selectedCommitOids.add(oid);
      selectionAnchorOid = oid;
    } else {
      selectedCommitOids = new Set([oid]);
      selectionAnchorOid = oid;
    }
    if (selectedCommitOids.size === 0) selectionAnchorOid = null;
    selectedCommitOids = new Set(visibleOids.filter((candidate) => selectedCommitOids.has(candidate)));
    runtime.detailRequestRevision += 1;
    this.update(runtime, {
      ...runtime.snapshot,
      selectedCommitOids,
      selectionAnchorOid,
      focusedCommitOid: selectedCommitOids.size > 0 ? oid : null,
      commitReview: null,
      commitReachability: null,
      historyDetailLoading: selectedCommitOids.size > 0,
    });
    void this.loadCommitReview(projectId, workspacePath, [...selectedCommitOids]);
  }

  setRepositoryTab(projectId: string, workspacePath: string | null | undefined, repositoryTab: SourceControlRepositoryTab) {
    const runtime = this.runtime(projectId, workspacePath);
    if (runtime.snapshot.repositoryTab === repositoryTab) return;
    this.update(runtime, { ...runtime.snapshot, repositoryTab });
  }

  selectCommitForContextMenu(projectId: string, workspacePath: string | null | undefined, oid: string) {
    const runtime = this.runtime(projectId, workspacePath);
    if (runtime.snapshot.selectedCommitOids.has(oid)) return;
    runtime.detailRequestRevision += 1;
    this.update(runtime, {
      ...runtime.snapshot,
      selectedCommitOids: new Set([oid]),
      selectionAnchorOid: oid,
      focusedCommitOid: oid,
      commitReview: null,
      commitReachability: null,
      historyDetailLoading: true,
    });
    void this.loadCommitReview(projectId, workspacePath, [oid]);
  }

  clearCommitSelection(projectId: string, workspacePath?: string | null) {
    const runtime = this.runtime(projectId, workspacePath);
    runtime.detailRequestRevision += 1;
    this.update(runtime, {
      ...runtime.snapshot,
      selectedCommitOids: new Set(),
      selectionAnchorOid: null,
      focusedCommitOid: null,
      commitReview: null,
      commitReachability: null,
      historyDetailLoading: false,
      reachabilityLoading: false,
    });
  }

  setSubject(projectId: string, workspacePath: string | null | undefined, subject: string) {
    const runtime = this.runtime(projectId, workspacePath);
    if (runtime.snapshot.subject === subject) return;
    this.update(runtime, { ...runtime.snapshot, subject });
  }

  setBody(projectId: string, workspacePath: string | null | undefined, body: string) {
    const runtime = this.runtime(projectId, workspacePath);
    if (runtime.snapshot.body === body) return;
    this.update(runtime, { ...runtime.snapshot, body });
  }

  async mutate(
    projectId: string,
    workspacePath: string | null | undefined,
    input: GitMutationRequestVm,
  ) {
    const runtime = this.runtime(projectId, workspacePath);
    const workspaceScopePath = runtime.snapshot.requestedWorkspacePath;
    const snapshot = runtime.snapshot.snapshot;
    if (!snapshot || runtime.snapshot.pendingAction) return;
    if (!isWorkspaceMutation(input) && !runtime.snapshot.catalog) return;
    const requestRevision = ++runtime.repositoryRequestRevision;
    runtime.catalogRequestRevision += 1;
    runtime.historyRequestRevision += 1;
    runtime.detailRequestRevision += 1;
    this.update(runtime, {
      ...runtime.snapshot,
      pendingAction: pendingActionFromMutation(input),
      activeOperation: null,
      error: null,
    });
    let mutationApplied = false;
    try {
      const result = await this.api.executeMutation(projectId, workspaceScopePath, {
        ...input,
        expectedRevision: isWorkspaceMutation(input)
          ? snapshot.repository.revision
          : runtime.snapshot.catalog?.catalogRevision,
      });
      mutationApplied = true;
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      if (result.scope === 'workspace') {
        runtime.pendingDiffAll = true;
        this.update(runtime, {
          ...runtime.snapshot,
          snapshot: {
            ...snapshot,
            repository: { ...snapshot.repository, revision: result.repositoryRevision },
            status: result.status,
          },
          pendingAction: null,
          error: null,
        });
        this.applyWorkspaceProjection(runtime, result.status);
        this.loadVisibleDetails(runtime);
        return;
      }
      const [nextSnapshot, history] = await Promise.all([
        this.api.getSnapshot(projectId, workspaceScopePath),
        this.historyIsCurrent(runtime)
          ? this.api.getHistory(projectId, workspaceScopePath, { limit: HISTORY_PAGE_SIZE })
          : Promise.resolve(runtime.snapshot.history),
      ]);
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      this.registerCanonicalAlias(runtime, nextSnapshot.repository.workspacePath);
      runtime.pendingDiffAll = true;
      this.update(runtime, {
        ...resetHistoryState(runtime.snapshot),
        status: 'ready',
        canonicalWorkspacePath: nextSnapshot.repository.workspacePath,
        snapshot: nextSnapshot,
        history,
        pendingAction: null,
        error: null,
        subject: input.kind === 'commit' ? '' : runtime.snapshot.subject,
        body: input.kind === 'commit' ? '' : runtime.snapshot.body,
      });
      this.applyWorkspaceProjection(runtime, nextSnapshot.status);
      this.loadVisibleDetails(runtime);
      this.loadStatistics(runtime);
    } catch (reason) {
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      const error = structuredErrorFrom(reason, mutationApplied ? 'git.status-failed' : 'git.operation-failed');
      this.update(runtime, {
        ...runtime.snapshot,
        pendingAction: null,
        error,
      });
      if (isRevisionChangedError(error.code)) {
        void this.load(projectId, workspaceScopePath, true, false, 'background');
      }
    }
  }

  async loadMoreHistory(projectId: string, workspacePath: string | null | undefined, advancePage: boolean) {
    const runtime = this.runtime(projectId, workspacePath);
    const workspaceScopePath = runtime.snapshot.requestedWorkspacePath;
    const history = runtime.snapshot.history;
    if (!history?.nextCursor || runtime.snapshot.pendingAction) return;
    const requestRevision = ++runtime.historyRequestRevision;
    this.update(runtime, { ...runtime.snapshot, pendingAction: { kind: 'history-more', path: null }, error: null });
    try {
      const page = await this.api.getHistory(projectId, workspaceScopePath, {
        cursor: history.nextCursor,
        limit: HISTORY_PAGE_SIZE,
        revision: history.revision,
      });
      if (runtime.historyRequestRevision !== requestRevision) return;
      if (advancePage) runtime.historyCommitScrollTop = 0;
      this.update(runtime, {
        ...runtime.snapshot,
        history: {
          commits: [...history.commits, ...page.commits],
          nextCursor: page.nextCursor,
          revision: page.revision,
        },
        historyPage: advancePage ? runtime.snapshot.historyPage + 1 : runtime.snapshot.historyPage,
        pendingAction: null,
      });
    } catch (reason) {
      if (runtime.historyRequestRevision !== requestRevision) return;
      this.update(runtime, {
        ...runtime.snapshot,
        pendingAction: null,
        error: structuredErrorFrom(reason, 'git.history-query-failed'),
      });
    }
  }

  private async loadCommitReview(
    projectId: string,
    workspacePath: string | null | undefined,
    selectedOids: string[],
  ) {
    const runtime = this.runtime(projectId, workspacePath);
    const workspaceScopePath = runtime.snapshot.requestedWorkspacePath;
    const history = runtime.snapshot.history;
    if (!history || selectedOids.length === 0) return;
    const requestRevision = ++runtime.detailRequestRevision;
    const cacheKey = commitReviewCacheKey(runtime.storageKey, history.revision, selectedOids);
    let slot = this.commitReviews.get(cacheKey);
    if (!slot) {
      slot = {};
      this.commitReviews.set(cacheKey, slot);
      while (this.commitReviews.size > 48) {
        const oldest = this.commitReviews.keys().next().value as string | undefined;
        if (!oldest) break;
        this.commitReviews.delete(oldest);
      }
    }
    try {
      const request = slot.value
        ? Promise.resolve(slot.value)
        : slot.request ?? this.api.getCommitReview(projectId, workspaceScopePath, {
          selectedOids,
          revision: history.revision,
        });
      slot.request = request;
      const commitReview = await request;
      slot.value = commitReview;
      slot.request = undefined;
      if (runtime.detailRequestRevision !== requestRevision) return;
      this.update(runtime, { ...runtime.snapshot, commitReview, historyDetailLoading: false, reviewStatisticsLoading: false, reviewStatisticsError: null });
      if (this.api.getCommitReviewStatistics && !slot.statisticsReady) {
        this.update(runtime, { ...runtime.snapshot, reviewStatisticsLoading: true });
        const statistics = slot.statistics ?? this.api.getCommitReviewStatistics(projectId, workspaceScopePath, commitReview);
        slot.statistics = statistics;
        try {
          const completed = await statistics;
          slot.value = completed;
          slot.statisticsReady = true;
          diffReviewStore.publishCommitStatistics(projectId, workspaceScopePath, completed.files);
          if (runtime.detailRequestRevision !== requestRevision) return;
          this.update(runtime, { ...runtime.snapshot, commitReview: completed, reviewStatisticsLoading: false });
        } catch (reason) {
          if (runtime.detailRequestRevision !== requestRevision) return;
          this.update(runtime, { ...runtime.snapshot, reviewStatisticsLoading: false,
            reviewStatisticsError: structuredErrorFrom(reason, 'git.commit-review-query-failed') });
        } finally { slot.statistics = undefined; }
      }
    } catch (reason) {
      slot.request = undefined;
      if (!slot.value) this.commitReviews.delete(cacheKey);
      if (runtime.detailRequestRevision !== requestRevision) return;
      this.update(runtime, {
        ...runtime.snapshot,
        historyDetailLoading: false,
        error: structuredErrorFrom(reason, 'git.commit-review-query-failed'),
      });
    }
  }

  retryReviewStatistics(projectId: string, workspacePath?: string | null) {
    const runtime = this.runtime(projectId, workspacePath);
    return this.loadCommitReview(projectId, workspacePath, [...runtime.snapshot.selectedCommitOids]);
  }

  async loadCommitReachability(projectId: string, workspacePath: string | null | undefined, oid: string) {
    const runtime = this.runtime(projectId, workspacePath);
    const workspaceScopePath = runtime.snapshot.requestedWorkspacePath;
    const snapshot = runtime.snapshot.snapshot;
    if (!snapshot) return;
    const requestRevision = ++runtime.reachabilityRequestRevision;
    this.update(runtime, {
      ...runtime.snapshot,
      commitReachability: null,
      reachabilityLoading: true,
      error: null,
    });
    try {
      const commitReachability = await this.api.getCommitReachability(projectId, workspaceScopePath, {
        oid,
        targetRef: snapshot.repository.currentBranch ?? 'HEAD',
      });
      if (runtime.reachabilityRequestRevision !== requestRevision) return;
      this.update(runtime, { ...runtime.snapshot, commitReachability, reachabilityLoading: false });
    } catch (reason) {
      if (runtime.reachabilityRequestRevision !== requestRevision) return;
      this.update(runtime, {
        ...runtime.snapshot,
        reachabilityLoading: false,
        error: structuredErrorFrom(reason, 'git.commit-reachability-query-failed'),
      });
    }
  }

  closeCommitReachability(projectId: string, workspacePath?: string | null) {
    const runtime = this.runtime(projectId, workspacePath);
    runtime.reachabilityRequestRevision += 1;
    this.update(runtime, {
      ...runtime.snapshot,
      reachabilityLoading: false,
      commitReachability: null,
    });
  }

  async startOperation(
    projectId: string,
    workspacePath: string | null | undefined,
    input: GitOperationRequestVm,
  ) {
    const runtime = this.runtime(projectId, workspacePath);
    const workspaceScopePath = runtime.snapshot.requestedWorkspacePath;
    const snapshot = runtime.snapshot.snapshot;
    if (!snapshot || runtime.snapshot.pendingAction) return;
    this.update(runtime, {
      ...runtime.snapshot,
      pendingAction: { kind: input.kind, path: null },
      activeOperation: null,
      error: null,
    });
    try {
      await this.ensureSubscriptions();
      const activeOperation = await this.api.startOperation(projectId, workspaceScopePath, {
        ...input,
        expectedRevision: operationUsesSyncRevision(input.kind)
          ? snapshot.repository.syncRevision
          : snapshot.repository.revision,
      });
      const latestOperation = this.earlyOperationUpdates.get(activeOperation.operationId) ?? activeOperation;
      this.earlyOperationUpdates.delete(activeOperation.operationId);
      this.update(runtime, { ...runtime.snapshot, activeOperation: latestOperation });
      if (!isOperationPending(latestOperation)) void this.finishOperation(runtime, latestOperation);
    } catch (reason) {
      const error = structuredErrorFrom(reason, 'git.operation-failed');
      this.update(runtime, {
        ...runtime.snapshot,
        pendingAction: null,
        error,
      });
      if (isRevisionChangedError(error.code)) {
        void this.load(projectId, workspaceScopePath, true, false, 'background');
      }
    }
  }

  async cancelOperation(projectId: string, workspacePath?: string | null) {
    const runtime = this.runtime(projectId, workspacePath);
    const operation = runtime.snapshot.activeOperation;
    if (!operation?.cancelable) return;
    try {
      const activeOperation = await this.api.cancelOperation(operation.operationId);
      this.update(runtime, { ...runtime.snapshot, activeOperation });
      if (!isOperationPending(activeOperation)) void this.finishOperation(runtime, activeOperation);
    } catch (reason) {
      this.update(runtime, { ...runtime.snapshot, error: structuredErrorFrom(reason, 'git.operation-failed') });
    }
  }

  clear(projectId: string, workspacePath?: string | null) {
    const routeKey = workspaceRootKey(projectId, workspacePath);
    const storageKey = this.aliases.get(routeKey) ?? routeKey;
    const runtime = this.sessions.get(storageKey);
    if (runtime) this.disposeRuntime(runtime);
    this.sessions.delete(storageKey);
    this.clearCommitReviews(storageKey);
    for (const [alias, target] of this.aliases) {
      if (target === storageKey) this.aliases.delete(alias);
    }
  }

  private historyIsCurrent(runtime: SessionRuntime) {
    return runtime.snapshot.history != null || runtime.snapshot.activeTab === 'history';
  }

  private ensureHistory(projectId: string, workspacePath: string | null | undefined) {
    const runtime = this.runtime(projectId, workspacePath);
    const workspaceScopePath = runtime.snapshot.requestedWorkspacePath;
    if (runtime.snapshot.history || runtime.snapshot.status !== 'ready') {
      if (runtime.snapshot.historyLoading && runtime.snapshot.history) {
        this.update(runtime, { ...runtime.snapshot, historyLoading: false });
      }
      return Promise.resolve();
    }
    if (runtime.historyPromise) return runtime.historyPromise;
    const requestRevision = ++runtime.historyRequestRevision;
    if (!runtime.snapshot.historyLoading) {
      this.update(runtime, { ...runtime.snapshot, historyLoading: true });
    }
    const request = (async () => {
      const page = await this.api.getHistory(projectId, workspaceScopePath, { limit: HISTORY_PAGE_SIZE });
      if (runtime.historyRequestRevision !== requestRevision) return;
      this.update(runtime, { ...runtime.snapshot, history: page, historyLoading: false });
    })().catch((reason: unknown) => {
      if (runtime.historyRequestRevision !== requestRevision) return;
      this.update(runtime, {
        ...runtime.snapshot,
        historyLoading: false,
        error: structuredErrorFrom(reason, 'git.history-query-failed'),
      });
    }).finally(() => {
      if (runtime.historyPromise === request) runtime.historyPromise = null;
    });
    runtime.historyPromise = request;
    return request;
  }

  private async load(
    projectId: string,
    workspacePath: string | null | undefined,
    force: boolean,
    resetNavigation = force,
    refreshKind: SourceControlRefreshKind | null = force ? 'manual' : null,
  ) {
    const runtime = this.runtime(projectId, workspacePath);
    if (!force && runtime.snapshot.status === 'ready') {
      this.reportWatch({ event: 'session-reuse', projectId, monitorStarted: runtime.monitorStarted });
      await this.ensureSubscriptions();
      if (await this.startMonitor(runtime, workspacePath)) this.scheduleInvalidation(runtime, 'worktree');
      return;
    }
    if (!force && runtime.snapshot.status === 'unavailable') return;
    if (runtime.loadPromise && refreshKind !== 'manual') return runtime.loadPromise;
    if (runtime.snapshot.pendingAction) return;
    const diagnostic = createSourceControlLoadDiagnostic(!runtime.snapshot.snapshot, this.api.reportLoad);
    let outcome: "ready" | "unavailable" | "error" | "superseded" = "superseded";
    const requestRevision = ++runtime.repositoryRequestRevision;
    runtime.catalogRequestRevision += 1;
    runtime.historyRequestRevision += 1;
    runtime.detailRequestRevision += 1;
    const operationError = refreshKind === 'background'
      ? runtime.snapshot.error
      : null;
    this.update(runtime, {
      ...runtime.snapshot,
      status: runtime.snapshot.snapshot ? 'ready' : 'loading',
      refreshing: refreshKind,
      error: operationError,
    });
    const request = (async () => {
      const bootstrap = !runtime.snapshot.snapshot && this.api.getBootstrap
        ? await (async () => {
            await diagnostic.measure("subscriptionsMs", () => this.ensureSubscriptions());
            return diagnostic.measure(
              "snapshotMs",
              () => this.api.getBootstrap!(projectId, workspacePath, diagnostic.loadId),
            );
          })()
        : null;
      if (bootstrap) {
        const originalWorkspacePath = workspacePath ?? null;
        this.registerWorkspaceAlias(runtime, originalWorkspacePath);
        this.registerWorkspaceAlias(runtime, bootstrap.workspaceScopePath);
        this.registerWorkspaceAlias(runtime, bootstrap.overview?.repository.workspacePath);
        if (runtime.snapshot.requestedWorkspacePath !== bootstrap.workspaceScopePath) {
          this.update(runtime, {
            ...runtime.snapshot,
            requestedWorkspacePath: bootstrap.workspaceScopePath,
          });
        }
      }
      const workspaceScopePath = bootstrap
        ? bootstrap.workspaceScopePath
        : runtime.snapshot.requestedWorkspacePath;
      const currentCapability = runtime.snapshot.capability;
      const shouldProbe = !bootstrap && (!currentCapability
        || (refreshKind === 'manual' && !runtime.snapshot.snapshot)
        || (currentCapability.status !== 'ready' && currentCapability.status !== 'head-required'));
      const capability = bootstrap?.capability ?? (shouldProbe
        ? await diagnostic.measure("capabilityMs", () => this.api.getCapability(projectId, diagnostic.loadId))
        : currentCapability!);
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      if (capability.status !== 'ready' && capability.status !== 'head-required') {
        outcome = 'unavailable';
        this.update(runtime, {
          ...runtime.snapshot,
          status: 'unavailable',
          capability,
          remotesLoading: false,
          remotesError: null,
          refreshing: null,
          error: null,
        });
        return;
      }
      if (bootstrap?.overview) runtime.monitorStarted = true;
      if (!runtime.monitorStarted) {
        await diagnostic.measure("subscriptionsMs", () => this.ensureSubscriptions());
        const monitor = diagnostic.measure("monitorMs", () => this.startMonitor(runtime, workspaceScopePath, diagnostic.loadId));
        let snapshotStarted = false;
        // Subscribe before reading, but do not put native watcher setup on the
        // first-content path. Registration overlapping a read needs a catch-up.
        void runtime.monitorStartPromise?.then(() => {
          if (snapshotStarted && this.sessions.get(runtime.storageKey) === runtime) {
            this.scheduleInvalidation(runtime, 'repository');
          }
        }).catch(() => undefined);
        await Promise.resolve();
        snapshotStarted = true;
        void monitor;
        if (runtime.repositoryRequestRevision !== requestRevision) return;
      }
      const includeHistory = this.historyIsCurrent(runtime);
      const [snapshot, history] = await Promise.all([
        bootstrap?.overview
          ? Promise.resolve(bootstrap.overview)
          : diagnostic.measure("snapshotMs", () => this.api.getSnapshot(projectId, workspaceScopePath, diagnostic.loadId)),
        includeHistory
          ? this.api.getHistory(projectId, workspaceScopePath, { limit: HISTORY_PAGE_SIZE })
          : Promise.resolve(runtime.snapshot.history),
      ]);
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      const currentRepository = runtime.snapshot.snapshot?.repository;
      const remotes = currentRepository?.commonDir === snapshot.repository.commonDir
        ? currentRepository.remotes
        : snapshot.repository.remotes;
      const publishedSnapshot = {
        ...snapshot,
        status: retainVisibleWorkspaceStatistics(runtime.snapshot.snapshot, snapshot),
        repository: { ...snapshot.repository, remotes },
      };
      this.registerCanonicalAlias(runtime, publishedSnapshot.repository.workspacePath);
      this.update(runtime, {
        ...(resetNavigation ? resetHistoryState(runtime.snapshot) : runtime.snapshot),
        status: 'ready',
        capability,
        canonicalWorkspacePath: publishedSnapshot.repository.workspacePath,
        snapshot: publishedSnapshot,
        catalog: currentRepository
          && sameWorkspacePath(currentRepository.commonDir, publishedSnapshot.repository.commonDir)
          && sameWorkspacePath(currentRepository.workspacePath, publishedSnapshot.repository.workspacePath)
          ? runtime.snapshot.catalog : null,
        remotesLoading: Boolean(this.api.getRemotes),
        remotesError: null,
        history: includeHistory ? history : runtime.snapshot.history,
        historyLoading: includeHistory ? false : runtime.snapshot.historyLoading,
        refreshing: null,
        error: runtime.snapshot.error,
      });
      outcome = 'ready';
      diagnostic.finish(outcome);
      this.applyWorkspaceProjection(runtime, publishedSnapshot.status);
      this.loadVisibleDetails(runtime);
      void this.loadRemotes(runtime, diagnostic.loadId);
      this.loadStatistics(runtime, diagnostic.loadId);
      if (!resetNavigation && includeHistory && runtime.snapshot.activeTab === 'history') {
        const selectedOids = [...runtime.snapshot.selectedCommitOids];
        if (selectedOids.length > 0) {
          this.update(runtime, {
            ...runtime.snapshot,
            commitReview: null,
            historyDetailLoading: true,
          });
          void this.loadCommitReview(projectId, workspaceScopePath, selectedOids);
        }
      }
    })().catch((reason: unknown) => {
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      outcome = "error";
      this.update(runtime, {
        ...runtime.snapshot,
        status: runtime.snapshot.snapshot ? 'ready' : 'error',
        refreshing: null,
        error: structuredErrorFrom(reason, 'git.status-failed'),
      });
      if (
        runtime.snapshot.activeTab === 'history'
        && runtime.snapshot.selectedCommitOids.size > 0
        && runtime.snapshot.commitReview === null
      ) {
        this.update(runtime, { ...runtime.snapshot, historyDetailLoading: false });
      }
    }).finally(() => {
      diagnostic.finish(outcome);
      if (runtime.loadPromise === request) {
        runtime.loadPromise = null;
        this.armInvalidation(runtime);
      }
    });
    runtime.loadPromise = request;
    return request;
  }

  private ensureSubscriptions() {
    if (this.subscriptionsPromise) return this.subscriptionsPromise;
    const subscriptions = [
      this.api.subscribeOperationUpdates?.((operation) => this.handleOperationUpdate(operation)),
      this.api.subscribeStateChanges?.((event) => this.handleStateChange(event)),
      this.api.subscribeWorkspaceChanges?.((event) => this.handleWorkspaceChange(event)),
    ].filter((subscription): subscription is Promise<() => void> => Boolean(subscription));
    this.subscriptionsPromise = Promise.all(subscriptions).then(() => {
      this.reportWatch({ event: 'subscriptions', success: true });
    }).catch(() => {
      this.reportWatch({ event: 'subscriptions', success: false });
    });
    return this.subscriptionsPromise;
  }

  private reportWatch(report: SourceControlWatchReport) {
    try {
      void this.api.reportWatch?.(report).catch(() => undefined);
    } catch { /* Diagnostics must not change watcher or refresh behavior. */ }
  }

  private queueWorkspaceEventDiagnostic(batch: WorkspaceEventDiagnosticBatch) {
    if (!this.api.reportWatch) return;
    const current = this.workspaceEventDiagnostics.get(batch.projectId);
    if (current) {
      current.receivedEvents += batch.receivedEvents;
      current.projectSessions += batch.projectSessions;
      current.routedSessions += batch.routedSessions;
      current.metadataFilteredSessions += batch.metadataFilteredSessions;
      current.outOfScopeSessions += batch.outOfScopeSessions;
      current.scopeMismatchSessions += batch.scopeMismatchSessions;
      current.nestedWorktreeFilteredSessions += batch.nestedWorktreeFilteredSessions;
      current.pathOutsideWorkspaceSessions += batch.pathOutsideWorkspaceSessions;
    } else {
      this.workspaceEventDiagnostics.set(batch.projectId, batch);
    }
    if (this.workspaceEventDiagnosticTimer) return;
    this.workspaceEventDiagnosticTimer = setTimeout(() => {
      this.workspaceEventDiagnosticTimer = null;
      const batches = [...this.workspaceEventDiagnostics.values()];
      this.workspaceEventDiagnostics.clear();
      for (const diagnostic of batches) {
        this.reportWatch({ event: 'workspace-events', ...diagnostic });
      }
    }, WATCH_DIAGNOSTIC_FLUSH_MS);
  }

  private handleOperationUpdate(operation: GitOperationVm) {
    const runtime = [...this.sessions.values()].find(
      (candidate) => candidate.snapshot.activeOperation?.operationId === operation.operationId,
    );
    if (!runtime) {
      this.earlyOperationUpdates.set(operation.operationId, operation);
      return;
    }
    this.update(runtime, { ...runtime.snapshot, activeOperation: operation });
    if (!isOperationPending(operation)) void this.finishOperation(runtime, operation);
  }

  private handleStateChange(event: GitStateChangedEventVm) {
    for (const runtime of this.sessions.values()) {
      const repository = runtime.snapshot.snapshot?.repository;
      if (
        runtime.snapshot.projectId === event.projectId
        && (repository
          ? sameWorkspacePath(repository.commonDir, event.repositoryCommonDir)
            && sameWorkspacePath(repository.workspacePath, event.workspacePath)
          : sameWorkspacePath(
              runtime.snapshot.canonicalWorkspacePath ?? runtime.snapshot.requestedWorkspacePath ?? '',
              event.workspacePath,
            ))
      ) {
        runtime.pendingDiffAll = true;
        this.scheduleInvalidation(runtime, 'repository');
      }
    }
  }

  private handleWorkspaceChange(event: WorkspaceFileChangedEventVm) {
    const diagnostic: WorkspaceEventDiagnosticBatch = {
      projectId: event.projectId,
      receivedEvents: 1,
      projectSessions: 0,
      routedSessions: 0,
      metadataFilteredSessions: 0,
      outOfScopeSessions: 0,
      scopeMismatchSessions: 0,
      nestedWorktreeFilteredSessions: 0,
      pathOutsideWorkspaceSessions: 0,
    };
    const mainEventBelongsToLinkedWorkspace = event.workspacePath === null
      && [...this.sessions.values()].some((candidate) => (
        candidate.snapshot.projectId === event.projectId
        && candidate.snapshot.requestedWorkspacePath !== null
        && pathIsWithinWorkspace(event.canonicalPath, candidate.snapshot.requestedWorkspacePath)
      ));
    for (const runtime of this.sessions.values()) {
      if (runtime.snapshot.projectId !== event.projectId) continue;
      diagnostic.projectSessions += 1;
      // Each watch reports its work location; a worktree nested in the project
      // directory is otherwise seen by both the main and the worktree watch.
      if (!sameWorkspacePath(runtime.snapshot.requestedWorkspacePath ?? '', event.workspacePath ?? '')) {
        diagnostic.outOfScopeSessions += 1;
        diagnostic.scopeMismatchSessions += 1;
        continue;
      }
      if (mainEventBelongsToLinkedWorkspace && runtime.snapshot.requestedWorkspacePath === null) {
        diagnostic.outOfScopeSessions += 1;
        diagnostic.nestedWorktreeFilteredSessions += 1;
        continue;
      }
      const repository = runtime.snapshot.snapshot?.repository;
      const workspacePath = repository?.workspacePath
        ?? runtime.snapshot.canonicalWorkspacePath
        ?? runtime.snapshot.requestedWorkspacePath;
      if (!workspacePath || !pathIsWithinWorkspace(event.canonicalPath, workspacePath)) {
        diagnostic.outOfScopeSessions += 1;
        diagnostic.pathOutsideWorkspaceSessions += 1;
        continue;
      }
      const relativePath = workspaceRelativePath(workspacePath, event.canonicalPath);
      // Metadata has its own watcher. Routing it through the worktree watcher
      // bypasses transaction filtering and turns read-side locks into refreshes.
      if (relativePath === '.git') {
        diagnostic.routedSessions += 1;
        this.scheduleInvalidation(runtime, 'repository');
        continue;
      }
      if (relativePath?.startsWith('.git/')
        || (repository && pathIsWithinWorkspace(event.canonicalPath, repository.commonDir))) {
        diagnostic.metadataFilteredSessions += 1;
        continue;
      }
      diagnostic.routedSessions += 1;
      if (relativePath) runtime.pendingDiffPaths.add(relativePath);
      else runtime.pendingDiffAll = true;
      this.scheduleInvalidation(runtime, 'worktree');
    }
    this.queueWorkspaceEventDiagnostic(diagnostic);
  }

  private scheduleInvalidation(runtime: SessionRuntime, scope: SourceControlInvalidationScope) {
    if (scope === 'repository' || runtime.pendingInvalidation === null) {
      runtime.pendingInvalidation = scope;
    }
    runtime.invalidationStartedAt ??= Date.now();
    this.armInvalidation(runtime);
  }

  private armInvalidation(runtime: SessionRuntime) {
    if (!runtime.pendingInvalidation
      || runtime.loadPromise
      || runtime.snapshot.status !== 'ready'
      || runtime.snapshot.pendingAction) return;
    if (runtime.invalidationTimer) clearTimeout(runtime.invalidationTimer);
    const elapsed = Date.now() - (runtime.invalidationStartedAt ?? Date.now());
    const delay = Math.min(
      STATE_INVALIDATION_DEBOUNCE_MS,
      Math.max(0, STATE_INVALIDATION_MAX_LATENCY_MS - elapsed),
    );
    runtime.invalidationTimer = setTimeout(() => {
      runtime.invalidationTimer = null;
      if (runtime.snapshot.status !== 'ready' || runtime.snapshot.pendingAction || runtime.loadPromise) {
        this.armInvalidation(runtime);
        return;
      }
      const scope = runtime.pendingInvalidation;
      runtime.pendingInvalidation = null;
      runtime.invalidationStartedAt = null;
      if (scope === 'repository') {
        void this.load(
          runtime.snapshot.projectId,
          runtime.snapshot.canonicalWorkspacePath ?? runtime.snapshot.requestedWorkspacePath,
          true,
          false,
          'background',
        );
      } else if (scope === 'worktree') {
        void this.refreshWorktree(runtime);
      }
    }, delay);
  }

  private async refreshWorktree(runtime: SessionRuntime) {
    if (runtime.loadPromise || runtime.snapshot.status !== 'ready' || runtime.snapshot.pendingAction) {
      this.scheduleInvalidation(runtime, 'worktree');
      return;
    }
    const projectId = runtime.snapshot.projectId;
    const workspacePath = runtime.snapshot.requestedWorkspacePath;
    const requestRevision = ++runtime.repositoryRequestRevision;
    const diagnosticStarted = performance.now();
    let diagnosticOutcome: Extract<SourceControlWatchReport, { event: 'refresh-end' }>['outcome'] = 'superseded';
    this.reportWatch({
      event: 'refresh-start',
      projectId,
      pendingPaths: runtime.pendingDiffPaths.size,
      invalidateAll: runtime.pendingDiffAll,
    });
    this.update(runtime, { ...runtime.snapshot, refreshing: 'background' });
    const request = this.api.getSnapshot(projectId, workspacePath).then((snapshot) => {
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      diagnosticOutcome = 'ready';
      const currentRepository = runtime.snapshot.snapshot?.repository;
      const repositoryChanged = !currentRepository
        || !sameWorkspacePath(currentRepository.commonDir, snapshot.repository.commonDir)
        || !sameWorkspacePath(currentRepository.workspacePath, snapshot.repository.workspacePath);
      // File notifications include ignored files and generated logs. Only a
      // changed Git revision invalidates catalog writes; retain visible data.
      const statusChanged = repositoryChanged || currentRepository?.revision !== snapshot.repository.revision;
      if (statusChanged) runtime.catalogRequestRevision += 1;
      const publishedSnapshot = {
        ...snapshot,
        status: retainVisibleWorkspaceStatistics(runtime.snapshot.snapshot, snapshot),
        repository: {
          ...snapshot.repository,
          remotes: currentRepository?.commonDir === snapshot.repository.commonDir
            ? currentRepository.remotes
            : snapshot.repository.remotes,
        },
      };
      this.registerCanonicalAlias(runtime, publishedSnapshot.repository.workspacePath);
      this.update(runtime, {
        ...runtime.snapshot,
        canonicalWorkspacePath: publishedSnapshot.repository.workspacePath,
        snapshot: publishedSnapshot,
        catalog: repositoryChanged ? null : runtime.snapshot.catalog,
        refreshing: null,
      });
      this.applyWorkspaceProjection(runtime, publishedSnapshot.status);
      this.loadVisibleDetails(runtime);
      if (repositoryChanged) void this.loadRemotes(runtime);
      this.loadStatistics(runtime);
    }).catch((reason: unknown) => {
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      diagnosticOutcome = 'error';
      this.update(runtime, {
        ...runtime.snapshot,
        refreshing: null,
        error: structuredErrorFrom(reason, 'git.status-failed'),
      });
    }).finally(() => {
      this.reportWatch({
        event: 'refresh-end',
        projectId,
        outcome: diagnosticOutcome,
        elapsedMs: performance.now() - diagnosticStarted,
      });
      if (runtime.loadPromise === request) {
        runtime.loadPromise = null;
        this.armInvalidation(runtime);
      }
    });
    runtime.loadPromise = request;
    return request;
  }

  private async startMonitor(runtime: SessionRuntime, requestedWorkspacePath?: string | null, loadId?: string) {
    const workspacePath = requestedWorkspacePath === undefined
      ? runtime.snapshot.requestedWorkspacePath
      : requestedWorkspacePath;
    if (runtime.monitorStarted || !this.api.startMonitor) return false;
    runtime.monitorStarted = true;
    const request = loadId
      ? this.api.startMonitor(runtime.snapshot.projectId, workspacePath, loadId)
      : this.api.startMonitor(runtime.snapshot.projectId, workspacePath);
    runtime.monitorStartPromise = request;
    try {
      await request;
      return true;
    } catch {
      runtime.monitorStarted = false;
      return false;
    } finally {
      if (runtime.monitorStartPromise === request) runtime.monitorStartPromise = null;
    }
  }

  private disposeRuntime(runtime: SessionRuntime) {
    runtime.repositoryRequestRevision += 1;
    runtime.catalogRequestRevision += 1;
    runtime.historyRequestRevision += 1;
    runtime.detailRequestRevision += 1;
    if (runtime.invalidationTimer) clearTimeout(runtime.invalidationTimer);
    runtime.invalidationTimer = null;
    runtime.invalidationStartedAt = null;
    runtime.pendingInvalidation = null;
    if (runtime.monitorStarted && this.api.stopMonitor) {
      runtime.monitorStarted = false;
      const stop = () => this.api.stopMonitor?.(
          runtime.snapshot.projectId,
          runtime.snapshot.requestedWorkspacePath,
        );
      void (runtime.monitorStartPromise ?? Promise.resolve()).then(stop).catch(() => undefined);
    }
  }

  private async finishOperation(runtime: SessionRuntime, operation: GitOperationVm) {
    if (runtime.snapshot.activeOperation?.operationId !== operation.operationId) return;
    if (runtime.finishingOperationId === operation.operationId) return;
    runtime.finishingOperationId = operation.operationId;
    const operationError = operation.error ?? null;
    this.update(runtime, {
      ...runtime.snapshot,
      pendingAction: null,
      error: operationError ?? runtime.snapshot.error,
    });
    await this.load(
      runtime.snapshot.projectId,
      runtime.snapshot.requestedWorkspacePath,
      true,
      true,
      'background',
    );
    runtime.finishingOperationId = null;
  }

  private runtime(projectId: string, workspacePath: string | null | undefined, touch = true) {
    const routeKey = workspaceRootKey(projectId, workspacePath);
    const storageKey = this.aliases.get(routeKey) ?? routeKey;
    let runtime = this.sessions.get(storageKey);
    if (!runtime) {
      runtime = {
        storageKey,
        snapshot: idleSnapshot(projectId, workspacePath),
        listeners: new Set(),
        repositoryRequestRevision: 0,
        historyRequestRevision: 0,
        historyPromise: null,
        detailRequestRevision: 0,
        reachabilityRequestRevision: 0,
        loadPromise: null,
        catalogPromise: null,
        catalogRequestRevision: 0,
        catalogLoadedRevision: -1,
        statisticsPromise: null,
        remotesPromise: null,
        monitorStarted: false,
        monitorStartPromise: null,
        invalidationTimer: null,
        invalidationStartedAt: null,
        pendingInvalidation: null,
        pendingDiffPaths: new Set(),
        pendingDiffAll: false,
        finishingOperationId: null,
        historyCommitScrollTop: 0,
        historyReviewScrollTop: 0,
        historyReviewScrollKey: null,
      };
      this.sessions.set(storageKey, runtime);
      this.aliases.set(routeKey, storageKey);
      this.prune(storageKey);
    } else if (touch) {
      this.sessions.delete(storageKey);
      this.sessions.set(storageKey, runtime);
    }
    return runtime;
  }

  private applyWorkspaceProjection(runtime: SessionRuntime, status: GitSourceControlSnapshotVm['status']) {
    const invalidate = runtime.pendingDiffAll
      ? { all: true, paths: [] as string[] }
      : { all: false, paths: [...runtime.pendingDiffPaths] };
    runtime.pendingDiffAll = false;
    runtime.pendingDiffPaths.clear();
    diffReviewStore.publishWorkspaceRefresh({
      projectId: runtime.snapshot.projectId,
      workspacePath: runtime.snapshot.requestedWorkspacePath,
      staged: status.staged,
      unstaged: status.unstaged,
      untracked: status.untracked,
      invalidate,
    });
  }

  private registerCanonicalAlias(runtime: SessionRuntime, canonicalWorkspacePath: string) {
    this.registerWorkspaceAlias(runtime, canonicalWorkspacePath);
  }

  private registerWorkspaceAlias(runtime: SessionRuntime, workspacePath: string | null | undefined) {
    if (workspacePath === undefined) return;
    this.aliases.set(workspaceRootKey(runtime.snapshot.projectId, workspacePath), runtime.storageKey);
  }

  private prune(protectedStorageKey: string) {
    if (this.sessions.size <= SourceControlStore.MAX_SESSIONS) return;
    for (const [storageKey, runtime] of this.sessions) {
      if (this.sessions.size <= SourceControlStore.MAX_SESSIONS) break;
      if (storageKey === protectedStorageKey || runtime.listeners.size > 0 || runtime.snapshot.pendingAction) continue;
      this.disposeRuntime(runtime);
      this.sessions.delete(storageKey);
      this.clearCommitReviews(storageKey);
      for (const [alias, target] of this.aliases) {
        if (target === storageKey) this.aliases.delete(alias);
      }
    }
  }

  dismissOperationResult(projectId: string, workspacePath?: string | null) {
    const runtime = this.runtime(projectId, workspacePath);
    const operation = runtime.snapshot.activeOperation;
    if (!operation || isOperationPending(operation)) return;
    this.update(runtime, {
      ...runtime.snapshot,
      activeOperation: null,
      error: operation.error && sameStructuredError(runtime.snapshot.error, operation.error)
        ? null
        : runtime.snapshot.error,
    });
  }

  private clearCommitReviews(storageKey: string) {
    const prefix = `${storageKey}\u0000`;
    for (const key of this.commitReviews.keys()) {
      if (key.startsWith(prefix)) this.commitReviews.delete(key);
    }
  }

  private update(runtime: SessionRuntime, snapshot: SourceControlSessionSnapshot) {
    const actionFinished = runtime.snapshot.pendingAction !== null && snapshot.pendingAction === null;
    runtime.snapshot = snapshot;
    for (const listener of runtime.listeners) listener();
    if (actionFinished) this.armInvalidation(runtime);
  }
}

function idleSnapshot(projectId: string, workspacePath: string | null | undefined): SourceControlSessionSnapshot {
  return {
    projectId,
    requestedWorkspacePath: workspacePath ?? null,
    canonicalWorkspacePath: null,
    status: 'idle',
    capability: null,
    snapshot: null,
    catalog: null,
    catalogLoading: false,
    catalogError: null,
    remotesLoading: false,
    remotesError: null,
    history: null,
    historyLoading: false,
    activeTab: 'changes',
    repositoryTab: 'branches',
    historyPage: 0,
    selectedCommitOids: new Set(),
    selectionAnchorOid: null,
    focusedCommitOid: null,
    commitReview: null,
    commitReachability: null,
    historyDetailLoading: false,
    reachabilityLoading: false,
    error: null,
    pendingAction: null,
    refreshing: null,
    activeOperation: null,
    subject: '',
    body: '',
  };
}

function resetHistoryState(snapshot: SourceControlSessionSnapshot): SourceControlSessionSnapshot {
  return {
    ...snapshot,
    historyPage: 0,
    selectedCommitOids: new Set(),
    selectionAnchorOid: null,
    focusedCommitOid: null,
    commitReview: null,
    commitReachability: null,
    historyDetailLoading: false,
    reachabilityLoading: false,
  };
}

function commitReviewCacheKey(storageKey: string, revision: string, selectedOids: readonly string[]) {
  return `${storageKey}\u0000${revision}\u0000${selectedOids.join(',')}`;
}

function comparableWorkspacePath(workspacePath: string | null | undefined) {
  return normalizeWorkspacePath(workspacePath) ?? '__main__';
}

function sameWorkspacePath(left: string, right: string) {
  return comparableWorkspacePath(left) === comparableWorkspacePath(right);
}

function pathIsWithinWorkspace(path: string, workspacePath: string) {
  const candidate = comparableWorkspacePath(path);
  const root = comparableWorkspacePath(workspacePath);
  return candidate === root || candidate.startsWith(`${root}/`);
}

function workspaceRelativePath(workspacePath: string, canonicalPath: string) {
  const root = comparableWorkspacePath(workspacePath);
  const candidate = comparableWorkspacePath(canonicalPath);
  const prefix = `${root}/`;
  if (!candidate.startsWith(prefix)) return null;
  return candidate.slice(prefix.length);
}

// Overview reads intentionally omit numstat. Keep the last visible statistics
// while revalidating surviving rows; loadStatistics still replaces them, even
// when the porcelain revision is unchanged (it is not a content fingerprint).
function retainVisibleWorkspaceStatistics(
  previous: GitSourceControlOverviewVm | null,
  incoming: GitSourceControlOverviewVm,
): GitSourceControlOverviewVm['status'] {
  if (!previous
    || !sameWorkspacePath(previous.repository.commonDir, incoming.repository.commonDir)
    || !sameWorkspacePath(previous.repository.workspacePath, incoming.repository.workspacePath)
    || previous.repository.headOid !== incoming.repository.headOid) return incoming.status;
  const merge = (area: 'staged' | 'unstaged' | 'untracked' | 'conflicts') => {
    const prior = new Map(previous.status[area].map((change) => [change.path, change]));
    return incoming.status[area].map((change) => {
      const old = prior.get(change.path);
      if (!old || change.addedLines != null || change.deletedLines != null || change.binary
        || old.oldPath !== change.oldPath || old.kind !== change.kind
        || old.indexStatus !== change.indexStatus || old.worktreeStatus !== change.worktreeStatus
        || old.submodule !== change.submodule) return change;
      return { ...change, addedLines: old.addedLines, deletedLines: old.deletedLines };
    });
  };
  return { ...incoming.status, staged: merge('staged'), unstaged: merge('unstaged'), untracked: merge('untracked'), conflicts: merge('conflicts') };
}

function isWorkspaceMutation(input: GitMutationRequestVm) {
  return input.kind === 'commit' || input.kind === 'discard-path' || input.kind.startsWith('stage') || input.kind.startsWith('unstage');
}

function pendingActionFromMutation(input: GitMutationRequestVm): SourceControlPendingAction {
  const path = input.kind === 'worktree-remove' || input.kind === 'discard-path'
    ? input.path
    : (input.kind === 'stage-paths' || input.kind === 'unstage-paths') && input.paths.length === 1
      ? input.paths[0]
      : null;
  return { kind: input.kind, path };
}

function isOperationPending(operation: GitOperationVm) {
  return operation.status === 'queued' || operation.status === 'running';
}

const SYNC_OPERATION_KINDS = new Set<GitOperationRequestVm['kind']>(['fetch', 'pull', 'push', 'push-tag']);
const REVISION_CHANGED_CODES = new Set(['git.ref-changed', 'git.sync-ref-changed']);

function operationUsesSyncRevision(kind: GitOperationRequestVm['kind']) {
  return SYNC_OPERATION_KINDS.has(kind);
}

function isRevisionChangedError(code: string) {
  return REVISION_CHANGED_CODES.has(code);
}

function structuredErrorFrom(reason: unknown, fallback: string): GitOperationErrorVm {
  if (typeof reason !== 'object' || !reason) return { code: fallback, params: {} };
  const code = 'code' in reason && typeof reason.code === 'string' ? reason.code : fallback;
  const params = 'params' in reason && typeof reason.params === 'object' && reason.params && !Array.isArray(reason.params)
    ? reason.params as Record<string, unknown>
    : {};
  return { code, params };
}

function sameStructuredError(left: GitOperationErrorVm | null, right: GitOperationErrorVm) {
  if (!left || left.code !== right.code) return false;
  const leftKeys = Object.keys(left.params);
  const rightKeys = Object.keys(right.params);
  return leftKeys.length === rightKeys.length
    && leftKeys.every((key) => left.params[key] === right.params[key]);
}

export const sourceControlStore = new SourceControlStore();

export function useSourceControlSession(projectId: string, workspacePath?: string | null) {
  const subscribe = useCallback(
    (listener: () => void) => sourceControlStore.subscribe(projectId, workspacePath, listener),
    [projectId, workspacePath],
  );
  const getSnapshot = useCallback(
    () => sourceControlStore.session(projectId, workspacePath),
    [projectId, workspacePath],
  );
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
