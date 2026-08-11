import { useCallback, useSyncExternalStore } from 'react';
import {
  analyzeGitCommitRelations,
  cancelGitOperation,
  executeGitMutation,
  getGitCommitDetail,
  getGitHistory,
  getSourceControlSnapshot,
  startGitOperation,
  startGitStateMonitor,
  stopGitStateMonitor,
  subscribeGitOperationUpdates,
  subscribeGitStateChanges,
  subscribeWorkspaceFileChanges,
} from '@/api';
import type {
  GitCommitDetailVm,
  GitCommitRelationsQueryVm,
  GitCommitRelationsVm,
  GitHistoryPageVm,
  GitMutationRequestVm,
  GitOperationRequestVm,
  GitOperationVm,
  GitStateChangedEventVm,
  GitSourceControlSnapshotVm,
  WorkspaceFileChangedEventVm,
} from '@/types';

export type SourceControlTab = 'changes' | 'history' | 'repository' | 'github';
export type SourceControlHistoryDetailKind = 'commit' | 'relations';

export interface SourceControlSessionSnapshot {
  projectId: string;
  requestedWorkspacePath: string | null;
  canonicalWorkspacePath: string | null;
  status: 'idle' | 'loading' | 'ready' | 'error';
  snapshot: GitSourceControlSnapshotVm | null;
  history: GitHistoryPageVm | null;
  activeTab: SourceControlTab;
  historyPage: number;
  selectedCommitOids: ReadonlySet<string>;
  focusedCommitOid: string | null;
  commitDetail: GitCommitDetailVm | null;
  commitRelations: GitCommitRelationsVm | null;
  historyDetailKind: SourceControlHistoryDetailKind;
  historyDetailLoading: boolean;
  errorCode: string | null;
  pendingOperation: string | null;
  activeOperation: GitOperationVm | null;
  subject: string;
  body: string;
}

interface SourceControlApi {
  getSnapshot: typeof getSourceControlSnapshot;
  getHistory: typeof getGitHistory;
  getCommitDetail: typeof getGitCommitDetail;
  analyzeCommitRelations: typeof analyzeGitCommitRelations;
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
  loadPromise: Promise<void> | null;
  monitorStarted: boolean;
  monitorStartPromise: Promise<void> | null;
  invalidationTimer: ReturnType<typeof setTimeout> | null;
  finishingOperationId: string | null;
}

const DEFAULT_API: SourceControlApi = {
  getSnapshot: getSourceControlSnapshot,
  getHistory: getGitHistory,
  getCommitDetail: getGitCommitDetail,
  analyzeCommitRelations: analyzeGitCommitRelations,
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
    return this.load(projectId, workspacePath, true);
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

  toggleCommitSelection(projectId: string, workspacePath: string | null | undefined, oid: string) {
    const runtime = this.runtime(projectId, workspacePath);
    const selectedCommitOids = new Set(runtime.snapshot.selectedCommitOids);
    if (selectedCommitOids.has(oid)) selectedCommitOids.delete(oid);
    else selectedCommitOids.add(oid);
    runtime.detailRequestRevision += 1;
    this.update(runtime, {
      ...runtime.snapshot,
      selectedCommitOids,
      commitRelations: null,
      historyDetailLoading: false,
    });
  }

  clearCommitSelection(projectId: string, workspacePath?: string | null) {
    const runtime = this.runtime(projectId, workspacePath);
    runtime.detailRequestRevision += 1;
    this.update(runtime, {
      ...runtime.snapshot,
      selectedCommitOids: new Set(),
      commitRelations: null,
      historyDetailLoading: false,
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
    operation: string,
  ) {
    const runtime = this.runtime(projectId, workspacePath);
    const snapshot = runtime.snapshot.snapshot;
    if (!snapshot || runtime.snapshot.pendingOperation) return;
    const requestRevision = ++runtime.repositoryRequestRevision;
    runtime.historyRequestRevision += 1;
    runtime.detailRequestRevision += 1;
    this.update(runtime, { ...runtime.snapshot, pendingOperation: operation, errorCode: null });
    try {
      const result = await this.api.executeMutation(projectId, workspacePath, {
        ...input,
        expectedRevision: snapshot.repository.revision,
      });
      const history = await this.api.getHistory(projectId, workspacePath, { limit: HISTORY_PAGE_SIZE });
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      this.registerCanonicalAlias(runtime, result.snapshot.repository.workspacePath);
      this.update(runtime, {
        ...resetHistoryState(runtime.snapshot),
        status: 'ready',
        canonicalWorkspacePath: result.snapshot.repository.workspacePath,
        snapshot: result.snapshot,
        history,
        pendingOperation: null,
        errorCode: null,
        subject: input.kind === 'commit' ? '' : runtime.snapshot.subject,
        body: input.kind === 'commit' ? '' : runtime.snapshot.body,
      });
    } catch (reason) {
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      this.update(runtime, {
        ...runtime.snapshot,
        pendingOperation: null,
        errorCode: errorCodeFrom(reason, 'git.operation-failed'),
      });
    }
  }

  async loadMoreHistory(projectId: string, workspacePath: string | null | undefined, advancePage: boolean) {
    const runtime = this.runtime(projectId, workspacePath);
    const history = runtime.snapshot.history;
    if (!history?.nextCursor || runtime.snapshot.pendingOperation) return;
    const requestRevision = ++runtime.historyRequestRevision;
    this.update(runtime, { ...runtime.snapshot, pendingOperation: 'history-more', errorCode: null });
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
        pendingOperation: null,
      });
    } catch (reason) {
      if (runtime.historyRequestRevision !== requestRevision) return;
      this.update(runtime, {
        ...runtime.snapshot,
        pendingOperation: null,
        errorCode: errorCodeFrom(reason, 'git.history-query-failed'),
      });
    }
  }

  async openCommitDetail(projectId: string, workspacePath: string | null | undefined, oid: string) {
    const runtime = this.runtime(projectId, workspacePath);
    const requestRevision = ++runtime.detailRequestRevision;
    this.update(runtime, {
      ...runtime.snapshot,
      focusedCommitOid: oid,
      historyDetailKind: 'commit',
      commitDetail: null,
      commitRelations: null,
      historyDetailLoading: true,
      errorCode: null,
    });
    try {
      const commitDetail = await this.api.getCommitDetail(projectId, workspacePath, oid);
      if (runtime.detailRequestRevision !== requestRevision) return;
      this.update(runtime, { ...runtime.snapshot, commitDetail, historyDetailLoading: false });
    } catch (reason) {
      if (runtime.detailRequestRevision !== requestRevision) return;
      this.update(runtime, {
        ...runtime.snapshot,
        historyDetailLoading: false,
        errorCode: errorCodeFrom(reason, 'git.commit-detail-query-failed'),
      });
    }
  }

  async analyzeSelectedCommits(projectId: string, workspacePath?: string | null) {
    const runtime = this.runtime(projectId, workspacePath);
    const snapshot = runtime.snapshot.snapshot;
    if (!snapshot || runtime.snapshot.selectedCommitOids.size < 2) return;
    const requestRevision = ++runtime.detailRequestRevision;
    const query: GitCommitRelationsQueryVm = {
      selectedOids: [...runtime.snapshot.selectedCommitOids],
      targetRef: snapshot.repository.currentBranch ?? 'HEAD',
    };
    this.update(runtime, {
      ...runtime.snapshot,
      historyDetailKind: 'relations',
      commitDetail: null,
      commitRelations: null,
      historyDetailLoading: true,
      errorCode: null,
    });
    try {
      const commitRelations = await this.api.analyzeCommitRelations(projectId, workspacePath, query);
      if (runtime.detailRequestRevision !== requestRevision) return;
      this.update(runtime, { ...runtime.snapshot, commitRelations, historyDetailLoading: false });
    } catch (reason) {
      if (runtime.detailRequestRevision !== requestRevision) return;
      this.update(runtime, {
        ...runtime.snapshot,
        historyDetailLoading: false,
        errorCode: errorCodeFrom(reason, 'git.commit-relation-query-failed'),
      });
    }
  }

  closeHistoryDetail(projectId: string, workspacePath?: string | null) {
    const runtime = this.runtime(projectId, workspacePath);
    runtime.detailRequestRevision += 1;
    this.update(runtime, {
      ...runtime.snapshot,
      historyDetailLoading: false,
      commitDetail: null,
      commitRelations: null,
    });
  }

  async startOperation(
    projectId: string,
    workspacePath: string | null | undefined,
    input: GitOperationRequestVm,
    operation: string,
  ) {
    const runtime = this.runtime(projectId, workspacePath);
    const snapshot = runtime.snapshot.snapshot;
    if (!snapshot || runtime.snapshot.pendingOperation) return;
    this.update(runtime, { ...runtime.snapshot, pendingOperation: operation, errorCode: null });
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
        pendingOperation: null,
        errorCode: errorCodeFrom(reason, 'git.operation-failed'),
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
      this.update(runtime, { ...runtime.snapshot, errorCode: errorCodeFrom(reason, 'git.operation-failed') });
    }
  }

  clear(projectId: string, workspacePath?: string | null) {
    const routeKey = sessionRouteKey(projectId, workspacePath);
    const storageKey = this.aliases.get(routeKey) ?? routeKey;
    const runtime = this.sessions.get(storageKey);
    if (runtime) this.disposeRuntime(runtime);
    this.sessions.delete(storageKey);
    for (const [alias, target] of this.aliases) {
      if (target === storageKey) this.aliases.delete(alias);
    }
  }

  private async load(
    projectId: string,
    workspacePath: string | null | undefined,
    force: boolean,
    resetNavigation = force,
  ) {
    const runtime = this.runtime(projectId, workspacePath);
    if (!force && runtime.snapshot.status === 'ready') return;
    if (!force && runtime.loadPromise) return runtime.loadPromise;
    if (runtime.snapshot.pendingOperation && runtime.snapshot.pendingOperation !== 'refresh') return;
    const requestRevision = ++runtime.repositoryRequestRevision;
    runtime.historyRequestRevision += 1;
    runtime.detailRequestRevision += 1;
    this.update(runtime, {
      ...runtime.snapshot,
      status: runtime.snapshot.snapshot ? 'ready' : 'loading',
      pendingOperation: force ? 'refresh' : runtime.snapshot.pendingOperation,
      errorCode: null,
    });
    const request = Promise.all([
      this.api.getSnapshot(projectId, workspacePath),
      this.api.getHistory(projectId, workspacePath, { limit: HISTORY_PAGE_SIZE }),
    ]).then(([snapshot, history]) => {
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      this.registerCanonicalAlias(runtime, snapshot.repository.workspacePath);
      this.update(runtime, {
        ...(resetNavigation ? resetHistoryState(runtime.snapshot) : runtime.snapshot),
        status: 'ready',
        canonicalWorkspacePath: snapshot.repository.workspacePath,
        snapshot,
        history,
        pendingOperation: null,
        errorCode: null,
      });
      void this.startMonitor(runtime);
    }).catch((reason: unknown) => {
      if (runtime.repositoryRequestRevision !== requestRevision) return;
      this.update(runtime, {
        ...runtime.snapshot,
        status: runtime.snapshot.snapshot ? 'ready' : 'error',
        pendingOperation: null,
        errorCode: errorCodeFrom(reason, 'git.status-failed'),
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
      if (runtime.snapshot.status !== 'ready' || runtime.snapshot.pendingOperation) return;
      void this.load(
        runtime.snapshot.projectId,
        runtime.snapshot.canonicalWorkspacePath ?? runtime.snapshot.requestedWorkspacePath,
        true,
        false,
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
    const operationErrorCode = operation.error?.code ?? null;
    this.update(runtime, {
      ...runtime.snapshot,
      pendingOperation: null,
      errorCode: operationErrorCode ?? runtime.snapshot.errorCode,
    });
    await this.load(
      runtime.snapshot.projectId,
      runtime.snapshot.requestedWorkspacePath,
      true,
    );
    if (operationErrorCode && runtime.snapshot.activeOperation?.operationId === operation.operationId) {
      this.update(runtime, { ...runtime.snapshot, errorCode: operationErrorCode });
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
      if (storageKey === protectedStorageKey || runtime.listeners.size > 0 || runtime.snapshot.pendingOperation) continue;
      this.disposeRuntime(runtime);
      this.sessions.delete(storageKey);
      for (const [alias, target] of this.aliases) {
        if (target === storageKey) this.aliases.delete(alias);
      }
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
    snapshot: null,
    history: null,
    activeTab: 'changes',
    historyPage: 0,
    selectedCommitOids: new Set(),
    focusedCommitOid: null,
    commitDetail: null,
    commitRelations: null,
    historyDetailKind: 'commit',
    historyDetailLoading: false,
    errorCode: null,
    pendingOperation: null,
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
    focusedCommitOid: null,
    commitDetail: null,
    commitRelations: null,
    historyDetailKind: 'commit',
    historyDetailLoading: false,
  };
}

function sessionRouteKey(projectId: string, workspacePath: string | null | undefined) {
  return `${projectId}\u0000${normalizeWorkspacePath(workspacePath)}`;
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

function isOperationPending(operation: GitOperationVm) {
  return operation.status === 'queued' || operation.status === 'running';
}

function errorCodeFrom(reason: unknown, fallback: string) {
  return typeof reason === 'object' && reason && 'code' in reason && typeof reason.code === 'string'
    ? reason.code
    : fallback;
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
