import { useCallback, useSyncExternalStore } from 'react';
import {
  cancelGitOperation,
  executeGitMutation,
  getGitCapability,
  getGitCommitReachability,
  getGitCommitReview,
  getGitHistory,
  getSourceControlSnapshot,
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
  WorkspaceFileChangedEventVm,
} from '@/types';

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
  snapshot: GitSourceControlSnapshotVm | null;
  history: GitHistoryPageVm | null;
  activeTab: SourceControlTab;
  repositoryTab: SourceControlRepositoryTab;
  historyPage: number;
  selectedCommitOids: ReadonlySet<string>;
  selectionAnchorOid: string | null;
  focusedCommitOid: string | null;
  commitReview: GitCommitReviewVm | null;
  commitReachability: GitCommitReachabilityVm | null;
  historyDetailLoading: boolean;
  reachabilityLoading: boolean;
  error: GitOperationErrorVm | null;
  pendingAction: SourceControlPendingAction | null;
  refreshing: SourceControlRefreshKind | null;
  activeOperation: GitOperationVm | null;
  subject: string;
  body: string;
}

interface SourceControlApi {
  getCapability: typeof getGitCapability;
  initializeRepository: typeof initializeGitRepository;
  getSnapshot: typeof getSourceControlSnapshot;
  getHistory: typeof getGitHistory;
  getCommitReview: typeof getGitCommitReview;
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
  detailRequestRevision: number;
  reachabilityRequestRevision: number;
  loadPromise: Promise<void> | null;
  monitorStarted: boolean;
  monitorStartPromise: Promise<void> | null;
  invalidationTimer: ReturnType<typeof setTimeout> | null;
  finishingOperationId: string | null;
}

interface CommitReviewCacheSlot {
  value?: GitCommitReviewVm;
  request?: Promise<GitCommitReviewVm>;
}

const DEFAULT_API: SourceControlApi = {
  getCapability: getGitCapability,
  initializeRepository: initializeGitRepository,
  getSnapshot: getSourceControlSnapshot,
  getHistory: getGitHistory,
  getCommitReview: getGitCommitReview,
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

export class SourceControlStore {
  static readonly MAX_SESSIONS = 24;

  private readonly sessions = new Map<string, SessionRuntime>();
  private readonly aliases = new Map<string, string>();
  private readonly earlyOperationUpdates = new Map<string, GitOperationVm>();
  private readonly commitReviews = new Map<string, CommitReviewCacheSlot>();
  private subscriptionsPromise: Promise<void> | null = null;

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
    if (runtime.snapshot.activeTab === activeTab) return;
    this.update(runtime, { ...runtime.snapshot, activeTab });
  }

  setHistoryPage(projectId: string, workspacePath: string | null | undefined, historyPage: number) {
    const runtime = this.runtime(projectId, workspacePath);
    const next = Math.max(0, Math.floor(historyPage));
    if (runtime.snapshot.historyPage === next) return;
    this.update(runtime, { ...runtime.snapshot, historyPage: next });
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
    const snapshot = runtime.snapshot.snapshot;
    if (!snapshot || runtime.snapshot.pendingAction) return;
    const requestRevision = ++runtime.repositoryRequestRevision;
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
      const result = await this.api.executeMutation(projectId, workspacePath, {
        ...input,
        expectedRevision: snapshot.repository.revision,
      });
      mutationApplied = true;
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      if (result.scope === 'workspace') {
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
        return;
      }
      const [nextSnapshot, history] = await Promise.all([
        this.api.getSnapshot(projectId, workspacePath),
        this.api.getHistory(projectId, workspacePath, { limit: HISTORY_PAGE_SIZE }),
      ]);
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      this.registerCanonicalAlias(runtime, nextSnapshot.repository.workspacePath);
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
    } catch (reason) {
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      this.update(runtime, {
        ...runtime.snapshot,
        pendingAction: null,
        error: structuredErrorFrom(reason, mutationApplied ? 'git.status-failed' : 'git.operation-failed'),
      });
    }
  }

  async loadMoreHistory(projectId: string, workspacePath: string | null | undefined, advancePage: boolean) {
    const runtime = this.runtime(projectId, workspacePath);
    const history = runtime.snapshot.history;
    if (!history?.nextCursor || runtime.snapshot.pendingAction) return;
    const requestRevision = ++runtime.historyRequestRevision;
    this.update(runtime, { ...runtime.snapshot, pendingAction: { kind: 'history-more', path: null }, error: null });
    try {
      const page = await this.api.getHistory(projectId, workspacePath, {
        cursor: history.nextCursor,
        limit: HISTORY_PAGE_SIZE,
        revision: history.revision,
      });
      if (runtime.historyRequestRevision !== requestRevision) return;
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
        : slot.request ?? this.api.getCommitReview(projectId, workspacePath, {
          selectedOids,
          revision: history.revision,
        });
      slot.request = request;
      const commitReview = await request;
      slot.value = commitReview;
      slot.request = undefined;
      if (runtime.detailRequestRevision !== requestRevision) return;
      this.update(runtime, { ...runtime.snapshot, commitReview, historyDetailLoading: false });
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

  async loadCommitReachability(projectId: string, workspacePath: string | null | undefined, oid: string) {
    const runtime = this.runtime(projectId, workspacePath);
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
      const commitReachability = await this.api.getCommitReachability(projectId, workspacePath, {
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
      const activeOperation = await this.api.startOperation(projectId, workspacePath, {
        ...input,
        expectedRevision: snapshot.repository.revision,
      });
      const latestOperation = this.earlyOperationUpdates.get(activeOperation.operationId) ?? activeOperation;
      this.earlyOperationUpdates.delete(activeOperation.operationId);
      this.update(runtime, { ...runtime.snapshot, activeOperation: latestOperation });
      if (!isOperationPending(latestOperation)) void this.finishOperation(runtime, latestOperation);
    } catch (reason) {
      this.update(runtime, {
        ...runtime.snapshot,
        pendingAction: null,
        error: structuredErrorFrom(reason, 'git.operation-failed'),
      });
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
    const routeKey = sessionRouteKey(projectId, workspacePath);
    const storageKey = this.aliases.get(routeKey) ?? routeKey;
    const runtime = this.sessions.get(storageKey);
    if (runtime) this.disposeRuntime(runtime);
    this.sessions.delete(storageKey);
    this.clearCommitReviews(storageKey);
    for (const [alias, target] of this.aliases) {
      if (target === storageKey) this.aliases.delete(alias);
    }
  }

  private async load(
    projectId: string,
    workspacePath: string | null | undefined,
    force: boolean,
    resetNavigation = force,
    refreshKind: SourceControlRefreshKind | null = force ? 'manual' : null,
  ) {
    const runtime = this.runtime(projectId, workspacePath);
    if (!force && (runtime.snapshot.status === 'ready' || runtime.snapshot.status === 'unavailable')) return;
    if (runtime.loadPromise && refreshKind !== 'manual') return runtime.loadPromise;
    if (runtime.snapshot.pendingAction) return;
    const requestRevision = ++runtime.repositoryRequestRevision;
    runtime.historyRequestRevision += 1;
    runtime.detailRequestRevision += 1;
    const operationError = refreshKind === 'background'
      && runtime.snapshot.activeOperation?.error
      && sameStructuredError(runtime.snapshot.error, runtime.snapshot.activeOperation.error)
        ? runtime.snapshot.error
        : null;
    this.update(runtime, {
      ...runtime.snapshot,
      status: runtime.snapshot.snapshot ? 'ready' : 'loading',
      refreshing: refreshKind,
      error: operationError,
    });
    const request = (async () => {
      const currentCapability = runtime.snapshot.capability;
      const shouldProbe = !currentCapability
        || (refreshKind === 'manual' && !runtime.snapshot.snapshot)
        || (currentCapability.status !== 'ready' && currentCapability.status !== 'head-required');
      const capability = shouldProbe
        ? await this.api.getCapability(projectId)
        : currentCapability;
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      if (capability.status !== 'ready' && capability.status !== 'head-required') {
        this.update(runtime, {
          ...runtime.snapshot,
          status: 'unavailable',
          capability,
          refreshing: null,
          error: null,
        });
        return;
      }
      const [snapshot, history] = await Promise.all([
        this.api.getSnapshot(projectId, workspacePath),
        this.api.getHistory(projectId, workspacePath, { limit: HISTORY_PAGE_SIZE }),
      ]);
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      this.registerCanonicalAlias(runtime, snapshot.repository.workspacePath);
      this.update(runtime, {
        ...(resetNavigation ? resetHistoryState(runtime.snapshot) : runtime.snapshot),
        status: 'ready',
        capability,
        canonicalWorkspacePath: snapshot.repository.workspacePath,
        snapshot,
        history,
        refreshing: null,
        error: operationError,
      });
      void this.startMonitor(runtime);
    })().catch((reason: unknown) => {
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      this.update(runtime, {
        ...runtime.snapshot,
        status: runtime.snapshot.snapshot ? 'ready' : 'error',
        refreshing: null,
        error: structuredErrorFrom(reason, 'git.status-failed'),
      });
    }).finally(() => {
      if (runtime.loadPromise === request) runtime.loadPromise = null;
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
    this.subscriptionsPromise = Promise.all(subscriptions).then(() => undefined).catch(() => undefined);
    return this.subscriptionsPromise;
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
        repository
        && repository.projectId === event.projectId
        && sameWorkspacePath(repository.commonDir, event.repositoryCommonDir)
        && sameWorkspacePath(repository.workspacePath, event.workspacePath)
      ) {
        this.scheduleInvalidation(runtime);
      }
    }
  }

  private handleWorkspaceChange(event: WorkspaceFileChangedEventVm) {
    for (const runtime of this.sessions.values()) {
      const repository = runtime.snapshot.snapshot?.repository;
      if (
        repository
        && repository.projectId === event.projectId
        && pathIsWithinWorkspace(event.canonicalPath, repository.workspacePath)
      ) {
        this.scheduleInvalidation(runtime);
      }
    }
  }

  private scheduleInvalidation(runtime: SessionRuntime) {
    if (runtime.invalidationTimer) clearTimeout(runtime.invalidationTimer);
    runtime.invalidationTimer = setTimeout(() => {
      runtime.invalidationTimer = null;
      if (runtime.snapshot.status !== 'ready' || runtime.snapshot.pendingAction) return;
      void this.load(
        runtime.snapshot.projectId,
        runtime.snapshot.canonicalWorkspacePath ?? runtime.snapshot.requestedWorkspacePath,
        true,
        false,
        'background',
      );
    }, STATE_INVALIDATION_DEBOUNCE_MS);
  }

  private async startMonitor(runtime: SessionRuntime) {
    const workspacePath = runtime.snapshot.canonicalWorkspacePath;
    if (runtime.monitorStarted || !workspacePath || !this.api.startMonitor) return;
    runtime.monitorStarted = true;
    const request = this.api.startMonitor(runtime.snapshot.projectId, workspacePath);
    runtime.monitorStartPromise = request;
    try {
      await request;
    } catch {
      runtime.monitorStarted = false;
    } finally {
      if (runtime.monitorStartPromise === request) runtime.monitorStartPromise = null;
    }
  }

  private disposeRuntime(runtime: SessionRuntime) {
    if (runtime.invalidationTimer) clearTimeout(runtime.invalidationTimer);
    runtime.invalidationTimer = null;
    if (runtime.monitorStarted && this.api.stopMonitor) {
      runtime.monitorStarted = false;
      const stop = () => this.api.stopMonitor?.(
          runtime.snapshot.projectId,
          runtime.snapshot.canonicalWorkspacePath ?? runtime.snapshot.requestedWorkspacePath,
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
    if (operationError && runtime.snapshot.activeOperation?.operationId === operation.operationId) {
      this.update(runtime, { ...runtime.snapshot, error: operationError });
    }
    runtime.finishingOperationId = null;
  }

  private runtime(projectId: string, workspacePath: string | null | undefined, touch = true) {
    const routeKey = sessionRouteKey(projectId, workspacePath);
    const storageKey = this.aliases.get(routeKey) ?? routeKey;
    let runtime = this.sessions.get(storageKey);
    if (!runtime) {
      runtime = {
        storageKey,
        snapshot: idleSnapshot(projectId, workspacePath),
        listeners: new Set(),
        repositoryRequestRevision: 0,
        historyRequestRevision: 0,
        detailRequestRevision: 0,
        reachabilityRequestRevision: 0,
        loadPromise: null,
        monitorStarted: false,
        monitorStartPromise: null,
        invalidationTimer: null,
        finishingOperationId: null,
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

  private registerCanonicalAlias(runtime: SessionRuntime, canonicalWorkspacePath: string) {
    this.aliases.set(sessionRouteKey(runtime.snapshot.projectId, canonicalWorkspacePath), runtime.storageKey);
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
    runtime.snapshot = snapshot;
    for (const listener of runtime.listeners) listener();
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
    history: null,
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

function sessionRouteKey(projectId: string, workspacePath: string | null | undefined) {
  return `${projectId}\u0000${normalizeWorkspacePath(workspacePath)}`;
}

function commitReviewCacheKey(storageKey: string, revision: string, selectedOids: readonly string[]) {
  return `${storageKey}\u0000${revision}\u0000${selectedOids.join(',')}`;
}

function normalizeWorkspacePath(workspacePath: string | null | undefined) {
  if (!workspacePath) return '__main__';
  const normalized = workspacePath.replaceAll('\\', '/').replace(/\/$/u, '');
  return /^[a-z]:\//iu.test(normalized) ? normalized.toLowerCase() : normalized;
}

function sameWorkspacePath(left: string, right: string) {
  return normalizeWorkspacePath(left) === normalizeWorkspacePath(right);
}

function pathIsWithinWorkspace(path: string, workspacePath: string) {
  const candidate = normalizeWorkspacePath(path);
  const root = normalizeWorkspacePath(workspacePath);
  return candidate === root || candidate.startsWith(`${root}/`);
}

function pendingActionFromMutation(input: GitMutationRequestVm): SourceControlPendingAction {
  const path = input.kind === 'worktree-remove'
    ? input.path
    : (input.kind === 'stage-paths' || input.kind === 'unstage-paths') && input.paths.length === 1
      ? input.paths[0]
      : null;
  return { kind: input.kind, path };
}

function isOperationPending(operation: GitOperationVm) {
  return operation.status === 'queued' || operation.status === 'running';
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
