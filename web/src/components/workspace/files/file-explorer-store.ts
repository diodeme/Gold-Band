import { useSyncExternalStore } from 'react';
import {
  createWorkspaceEntry,
  deleteWorkspaceEntry,
  listWorkspaceDirectory,
  renameWorkspaceEntry,
  restoreWorkspaceEntry,
  searchWorkspaceFiles,
} from '@/api';
import type {
  WorkspaceDirectoryEntryVm,
  WorkspaceEntryKind,
  WorkspaceFileChangedEventVm,
  WorkspaceFileSearchVm,
  WorkspaceFilesVm,
} from '@/types';
import { FALLBACK_WORKSPACE_FILES } from '../workspace-layout';
import { fileContentStore } from './file-content-store';

/** Tree node id of the inline "new file / new folder" row. NUL never occurs in a relative path. */
export const FILE_TREE_DRAFT_ID = '\0draft';

export interface FileTreeDraft {
  parentRelativePath: string;
  kind: WorkspaceEntryKind;
}

/** A completed tree mutation, kept so Ctrl+Z can invert it. */
export type FileTreeOperation =
  | { kind: 'create'; entry: WorkspaceDirectoryEntryVm }
  | { kind: 'rename'; from: WorkspaceDirectoryEntryVm; to: WorkspaceDirectoryEntryVm }
  | { kind: 'delete'; receiptId: string; entry: WorkspaceDirectoryEntryVm };

/** Path identity changes other domains (open file selection) must follow. */
export type FileTreeEntryMutation =
  | { projectId: string; kind: 'removed'; entry: WorkspaceDirectoryEntryVm }
  | { projectId: string; kind: 'moved'; from: WorkspaceDirectoryEntryVm; to: WorkspaceDirectoryEntryVm };

export type FileTreeOperationResult =
  | { status: 'done'; entry: WorkspaceDirectoryEntryVm | null }
  | { status: 'failed'; errorCode: string; params: Record<string, unknown> }
  | { status: 'skipped' };

export interface FileTreeNode extends WorkspaceDirectoryEntryVm {
  id: string;
  children: FileTreeNode[] | null;
  loading: boolean;
}

export type FileTreeDisplayMode = 'compact' | 'tree';

export interface FileTreeViewNode extends FileTreeNode {
  displayName: string;
  children: FileTreeViewNode[] | null;
}

export interface FileExplorerSnapshot {
  projectId: string;
  status: 'idle' | 'loading' | 'ready' | 'error';
  roots: FileTreeNode[];
  expanded: ReadonlySet<string>;
  errorCode: string | null;
  searchQuery: string;
  searchStatus: 'idle' | 'loading' | 'ready' | 'error';
  searchResult: WorkspaceFileSearchVm | null;
  treeScrollTop: number;
  treeWidth: number | null;
  displayMode: FileTreeDisplayMode;
  draft: FileTreeDraft | null;
  /** Relative path of the entry a create/rename/delete/undo is writing; null when idle. */
  pendingPath: string | null;
}

interface ProjectRuntime {
  snapshot: FileExplorerSnapshot;
  operations: FileTreeOperation[];
  revealedSelectionPath: string | null;
  directoryRequests: Map<string, number>;
  searchRevision: number;
  searchTimer: ReturnType<typeof setTimeout> | null;
  refreshTimer: ReturnType<typeof setTimeout> | null;
  refreshStartedAt: number | null;
  refreshPromise: Promise<void> | null;
  refreshDirty: boolean;
  refreshAll: boolean;
  pendingRefreshDirectories: Set<string>;
}

function commandErrorCode(reason: unknown, fallback: string) {
  return typeof reason === 'object' && reason && 'code' in reason && typeof reason.code === 'string'
    ? reason.code
    : fallback;
}

function failedOperation(reason: unknown, fallback: string): FileTreeOperationResult {
  const params = typeof reason === 'object' && reason && 'params' in reason
    && typeof reason.params === 'object' && reason.params
    ? reason.params as Record<string, unknown>
    : {};
  return { status: 'failed', errorCode: commandErrorCode(reason, fallback), params };
}

function parentRelativePath(relativePath: string) {
  const index = relativePath.lastIndexOf('/');
  return index < 0 ? '' : relativePath.slice(0, index);
}

function draftNode(draft: FileTreeDraft): FileTreeNode {
  return {
    id: FILE_TREE_DRAFT_ID,
    name: '',
    relativePath: FILE_TREE_DRAFT_ID,
    canonicalPath: '',
    kind: draft.kind,
    hasChildren: false,
    byteLength: null,
    modifiedAtNs: null,
    children: [],
    loading: false,
  };
}

/** Folders go first and files after the last folder, matching the listing order. */
function insertDraft(children: readonly FileTreeNode[], draft: FileTreeDraft) {
  const index = draft.kind === 'directory'
    ? 0
    : children.findIndex((child) => child.kind !== 'directory');
  const at = index < 0 ? children.length : index;
  return [...children.slice(0, at), draftNode(draft), ...children.slice(at)];
}

export function fileTreeWithDraft(nodes: FileTreeNode[], draft: FileTreeDraft | null): FileTreeNode[] {
  if (!draft) return nodes;
  if (!draft.parentRelativePath) return insertDraft(nodes, draft);
  return updateNode(nodes, draft.parentRelativePath, (node) => (
    node.kind === 'directory' ? { ...node, children: insertDraft(node.children ?? [], draft) } : node
  ));
}

function remapRelativePath(path: string, from: string, to: string) {
  if (path === from) return to;
  return path.startsWith(`${from}/`) ? `${to}${path.slice(from.length)}` : path;
}

function sameDirectoryEntry(node: FileTreeNode, entry: WorkspaceDirectoryEntryVm) {
  return node.relativePath === entry.relativePath
    && node.kind === entry.kind
    && node.name === entry.name
    && node.canonicalPath === entry.canonicalPath;
}

function mergeDirectoryNodes(existing: readonly FileTreeNode[] | null, entries: readonly WorkspaceDirectoryEntryVm[]): FileTreeNode[] {
  if (
    existing
    && existing.length === entries.length
    && existing.every((node, index) => sameDirectoryEntry(node, entries[index]!))
  ) {
    return existing as FileTreeNode[];
  }
  const previous = new Map((existing ?? []).map((node) => [node.id, node]));
  return entries.map((entry) => {
    const prior = previous.get(entry.relativePath);
    if (prior && prior.kind === entry.kind) {
      if (sameDirectoryEntry(prior, entry)) return prior;
      return {
        ...prior,
        ...entry,
        id: entry.relativePath,
        children: prior.children,
        loading: prior.loading,
      };
    }
    return {
      ...entry,
      id: entry.relativePath,
      children: entry.kind === 'directory' ? null : [],
      loading: false,
    };
  });
}

function nodesFor(entries: WorkspaceDirectoryEntryVm[]): FileTreeNode[] {
  return mergeDirectoryNodes(null, entries);
}

function viewNodeFor(node: FileTreeNode, displayMode: FileTreeDisplayMode): FileTreeViewNode {
  if (displayMode === 'tree' || node.kind !== 'directory') {
    return {
      ...node,
      displayName: node.name,
      children: node.children === null
        ? null
        : node.children.map((child) => viewNodeFor(child, displayMode)),
    };
  }

  const chainHeadId = node.id;
  const names = [node.name];
  let tail = node;
  while (
    tail.kind === 'directory'
    && tail.children?.length === 1
    && tail.children[0]?.kind === 'directory'
    && tail.children[0].id !== FILE_TREE_DRAFT_ID
  ) {
    tail = tail.children[0];
    names.push(tail.name);
  }
  return {
    ...tail,
    id: chainHeadId,
    displayName: names.join('.'),
    children: tail.children === null
      ? null
      : tail.children.map((child) => viewNodeFor(child, displayMode)),
  };
}

export function fileTreeView(nodes: FileTreeNode[], displayMode: FileTreeDisplayMode) {
  return nodes.map((node) => viewNodeFor(node, displayMode));
}

function updateNode(nodes: FileTreeNode[], id: string, update: (node: FileTreeNode) => FileTreeNode): FileTreeNode[] {
  let changed = false;
  const next = nodes.map((node) => {
    if (node.id === id) {
      changed = true;
      return update(node);
    }
    if (!node.children?.length) return node;
    const children = updateNode(node.children, id, update);
    if (children === node.children) return node;
    changed = true;
    return { ...node, children };
  });
  return changed ? next : nodes;
}

function findNode(nodes: FileTreeNode[], id: string): FileTreeNode | null {
  for (const node of nodes) {
    if (node.id === id) return node;
    if (node.children) {
      const found = findNode(node.children, id);
      if (found) return found;
    }
  }
  return null;
}

function containsCanonicalPath(nodes: FileTreeNode[], canonicalPath: string): boolean {
  const target = normalizePath(canonicalPath);
  for (const node of nodes) {
    if (normalizePath(node.canonicalPath) === target) return true;
    if (node.children && containsCanonicalPath(node.children, canonicalPath)) return true;
  }
  return false;
}

export function fileChangeAffectsTree(
  snapshot: FileExplorerSnapshot,
  event: WorkspaceFileChangedEventVm,
): boolean {
  // File saves belong to the content domain. Atomic replacement can surface as
  // create/remove/rename events even though the visible path identity is stable.
  if (event.kind === 'modified' || event.operationId) return false;
  const knownPath = containsCanonicalPath(snapshot.roots, event.canonicalPath);
  if (event.kind === 'created') return !knownPath;
  if (event.kind === 'removed') return knownPath;
  if (event.kind === 'renamed') {
    // A path with a revision exists after the rename. Existing+present is an
    // atomic replacement; missing+present is a new tree node. Without a
    // revision the path no longer exists, so only a known node changed shape.
    return event.revision ? !knownPath : knownPath;
  }
  return true;
}

function idleSnapshot(projectId: string): FileExplorerSnapshot {
  return {
    projectId,
    status: 'idle',
    roots: [],
    expanded: new Set(),
    errorCode: null,
    searchQuery: '',
    searchStatus: 'idle',
    searchResult: null,
    treeScrollTop: 0,
    treeWidth: null,
    displayMode: 'compact',
    draft: null,
    pendingPath: null,
  };
}

export class FileExplorerStore {
  private static readonly MAX_PROJECTS = 24;
  /** Undo depth per project; history is session-only like the backend trash receipts. */
  private static readonly MAX_UNDO_OPERATIONS = 20;
  private static readonly DIRECTORY_CHAIN_EXPANSION_LIMIT = 64;
  private static readonly MAX_REFRESH_LATENCY_MS = 1_000;
  private static readonly MAX_PENDING_REFRESH_DIRECTORIES = 64;
  private config: WorkspaceFilesVm = FALLBACK_WORKSPACE_FILES;
  private readonly projects = new Map<string, ProjectRuntime>();
  private readonly listeners = new Set<() => void>();
  private readonly mutationListeners = new Set<(mutation: FileTreeEntryMutation) => void>();

  configure(config: WorkspaceFilesVm) {
    this.config = config;
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  subscribeEntryMutations(listener: (mutation: FileTreeEntryMutation) => void) {
    this.mutationListeners.add(listener);
    return () => { this.mutationListeners.delete(listener); };
  }

  canUndo(projectId: string) {
    return (this.projects.get(projectId)?.operations.length ?? 0) > 0;
  }

  /** Show an inline name editor for a new entry inside `parentRelativePath` ('' is the root). */
  async startDraft(projectId: string, parentRelativePath: string, kind: WorkspaceEntryKind) {
    const runtime = this.runtime(projectId);
    if (runtime.snapshot.pendingPath !== null) return;
    if (parentRelativePath && !runtime.snapshot.expanded.has(parentRelativePath)) {
      await this.toggleDirectory(projectId, parentRelativePath, true);
    } else if (parentRelativePath) {
      await this.loadDirectory(projectId, parentRelativePath);
    }
    this.setSnapshot(runtime, { ...runtime.snapshot, draft: { parentRelativePath, kind } });
  }

  cancelDraft(projectId: string) {
    const runtime = this.runtime(projectId);
    if (!runtime.snapshot.draft) return;
    this.setSnapshot(runtime, { ...runtime.snapshot, draft: null });
  }

  async createEntry(projectId: string, name: string): Promise<FileTreeOperationResult> {
    const runtime = this.runtime(projectId);
    const draft = runtime.snapshot.draft;
    if (!draft || runtime.snapshot.pendingPath !== null) return { status: 'skipped' };
    return this.runOperation(runtime, draft.parentRelativePath, async () => {
      try {
        const entry = await createWorkspaceEntry({
          projectId,
          parentRelativePath: draft.parentRelativePath,
          name,
          kind: draft.kind,
        });
        this.pushOperation(runtime, { kind: 'create', entry });
        return { status: 'done', entry };
      } finally {
        this.setSnapshot(runtime, { ...runtime.snapshot, draft: null });
      }
    }, [draft.parentRelativePath]);
  }

  async renameEntry(projectId: string, entry: WorkspaceDirectoryEntryVm, newName: string): Promise<FileTreeOperationResult> {
    const runtime = this.runtime(projectId);
    if (newName === entry.name || runtime.snapshot.pendingPath !== null) return { status: 'skipped' };
    return this.runOperation(runtime, entry.relativePath, async () => {
      const to = await this.renameWithContent(projectId, entry, newName);
      this.pushOperation(runtime, { kind: 'rename', from: entry, to });
      return { status: 'done', entry: to };
    }, [parentRelativePath(entry.relativePath)]);
  }

  async deleteEntry(projectId: string, entry: WorkspaceDirectoryEntryVm): Promise<FileTreeOperationResult> {
    const runtime = this.runtime(projectId);
    if (runtime.snapshot.pendingPath !== null) return { status: 'skipped' };
    return this.runOperation(runtime, entry.relativePath, async () => {
      const deletion = await deleteWorkspaceEntry(projectId, entry.relativePath);
      this.afterRemoved(runtime, deletion.entry);
      this.pushOperation(runtime, { kind: 'delete', receiptId: deletion.receiptId, entry: deletion.entry });
      return { status: 'done', entry: null };
    }, [parentRelativePath(entry.relativePath)]);
  }

  /** Invert the most recent operation. A failed undo is dropped, like a failed redo target in editors. */
  async undo(projectId: string): Promise<FileTreeOperationResult> {
    const runtime = this.runtime(projectId);
    const operation = runtime.operations.at(-1);
    if (!operation || runtime.snapshot.pendingPath !== null) return { status: 'skipped' };
    runtime.operations.pop();
    switch (operation.kind) {
      case 'create':
        return this.runOperation(runtime, operation.entry.relativePath, async () => {
          const deletion = await deleteWorkspaceEntry(projectId, operation.entry.relativePath);
          this.afterRemoved(runtime, deletion.entry);
          return { status: 'done', entry: null };
        }, [parentRelativePath(operation.entry.relativePath)]);
      case 'rename':
        return this.runOperation(runtime, operation.to.relativePath, async () => {
          const entry = await this.renameWithContent(projectId, operation.to, operation.from.name);
          return { status: 'done', entry };
        }, [parentRelativePath(operation.to.relativePath)]);
      case 'delete':
        return this.runOperation(runtime, operation.entry.relativePath, async () => {
          const entry = await restoreWorkspaceEntry(projectId, operation.receiptId);
          return { status: 'done', entry };
        }, [parentRelativePath(operation.entry.relativePath)]);
    }
  }

  private async renameWithContent(projectId: string, entry: WorkspaceDirectoryEntryVm, newName: string) {
    // Pending autosaves must land on the old path before it disappears.
    if (!await fileContentStore.flushWithin(projectId, entry.canonicalPath)) {
      throw { code: 'workspace-file.unsaved-changes', params: { path: entry.canonicalPath } };
    }
    const to = await renameWorkspaceEntry(projectId, entry.relativePath, newName);
    const runtime = this.runtime(projectId);
    const expanded = new Set([...runtime.snapshot.expanded].map((path) => remapRelativePath(path, entry.relativePath, to.relativePath)));
    this.setSnapshot(runtime, { ...runtime.snapshot, expanded });
    await fileContentStore.releaseWithin(projectId, entry.canonicalPath);
    this.emitMutation({ projectId, kind: 'moved', from: entry, to });
    return to;
  }

  private afterRemoved(runtime: ProjectRuntime, entry: WorkspaceDirectoryEntryVm) {
    const expanded = new Set([...runtime.snapshot.expanded].filter((path) => remapRelativePath(path, entry.relativePath, '') === path));
    this.setSnapshot(runtime, { ...runtime.snapshot, expanded });
    void fileContentStore.releaseWithin(runtime.snapshot.projectId, entry.canonicalPath);
    this.emitMutation({ projectId: runtime.snapshot.projectId, kind: 'removed', entry });
  }

  private pushOperation(runtime: ProjectRuntime, operation: FileTreeOperation) {
    runtime.operations.push(operation);
    if (runtime.operations.length > FileExplorerStore.MAX_UNDO_OPERATIONS) runtime.operations.shift();
  }

  private async runOperation(
    runtime: ProjectRuntime,
    pendingPath: string,
    operation: () => Promise<FileTreeOperationResult>,
    refreshDirectories: string[],
  ): Promise<FileTreeOperationResult> {
    this.setSnapshot(runtime, { ...runtime.snapshot, pendingPath });
    try {
      return await operation();
    } catch (reason) {
      return failedOperation(reason, 'workspace-file.write-failed');
    } finally {
      this.setSnapshot(runtime, { ...runtime.snapshot, pendingPath: null });
      await this.refreshDirectories(runtime.snapshot.projectId, refreshDirectories);
    }
  }

  /** Refresh mutated parents now instead of waiting for the debounced watcher event. */
  private refreshDirectories(projectId: string, directories: string[]) {
    const runtime = this.runtime(projectId);
    if (directories.some((directory) => !directory)) return this.runRefresh(projectId, true);
    for (const directory of directories) runtime.pendingRefreshDirectories.add(directory);
    return this.runRefresh(projectId);
  }

  private emitMutation(mutation: FileTreeEntryMutation) {
    for (const listener of this.mutationListeners) listener(mutation);
  }

  snapshot = (projectId: string) => this.runtime(projectId, false).snapshot;

  setTreeScrollTop(projectId: string, treeScrollTop: number) {
    const runtime = this.runtime(projectId);
    const next = Math.max(0, Math.round(treeScrollTop));
    if (runtime.snapshot.treeScrollTop === next) return;
    runtime.snapshot = { ...runtime.snapshot, treeScrollTop: next };
  }

  setTreeWidth(projectId: string, treeWidth: number) {
    const runtime = this.runtime(projectId);
    const next = Math.max(1, Math.round(treeWidth));
    if (runtime.snapshot.treeWidth === next) return;
    runtime.snapshot = { ...runtime.snapshot, treeWidth: next };
  }

  setDisplayMode(projectId: string, displayMode: FileTreeDisplayMode) {
    const runtime = this.runtime(projectId);
    if (runtime.snapshot.displayMode === displayMode) return;
    this.setSnapshot(runtime, { ...runtime.snapshot, displayMode });
  }

  takeSelectionReveal(projectId: string, canonicalPath: string | null) {
    const runtime = this.runtime(projectId);
    const next = canonicalPath ? normalizePath(canonicalPath) : null;
    if (runtime.revealedSelectionPath === next) return false;
    runtime.revealedSelectionPath = next;
    return next !== null;
  }

  async loadRoot(projectId: string, force = false) {
    const runtime = this.runtime(projectId);
    if (!force && (runtime.snapshot.status === 'loading' || runtime.snapshot.status === 'ready')) return;
    this.setSnapshot(runtime, { ...runtime.snapshot, status: 'loading', errorCode: null });
    try {
      const entries = await listWorkspaceDirectory(projectId, '');
      this.setSnapshot(runtime, { ...runtime.snapshot, status: 'ready', roots: nodesFor(entries), errorCode: null });
      const expanded = [...runtime.snapshot.expanded].sort((left, right) => pathDepth(left) - pathDepth(right));
      for (const id of expanded) await this.loadDirectory(projectId, id);
    } catch (reason) {
      this.setSnapshot(runtime, {
        ...runtime.snapshot,
        status: 'error',
        errorCode: commandErrorCode(reason, 'workspace-file.read-failed'),
      });
    }
  }

  reconcile(projectId: string) {
    const runtime = this.runtime(projectId);
    if (runtime.snapshot.status !== 'ready') return this.loadRoot(projectId);
    return this.runRefresh(projectId, true);
  }

  private async refreshRoot(projectId: string) {
    const runtime = this.runtime(projectId);
    if (runtime.snapshot.status !== 'ready') {
      await this.loadRoot(projectId, true);
      return;
    }
    try {
      const entries = await listWorkspaceDirectory(projectId, '');
      const roots = mergeDirectoryNodes(runtime.snapshot.roots, entries);
      if (roots !== runtime.snapshot.roots || runtime.snapshot.errorCode !== null) {
        this.setSnapshot(runtime, { ...runtime.snapshot, roots, errorCode: null });
      }
      const expanded = [...runtime.snapshot.expanded].sort((left, right) => pathDepth(left) - pathDepth(right));
      for (const id of expanded) await this.loadDirectory(projectId, id, true);
    } catch (reason) {
      this.setSnapshot(runtime, {
        ...runtime.snapshot,
        errorCode: commandErrorCode(reason, 'workspace-file.read-failed'),
      });
    }
  }

  async toggleDirectory(projectId: string, relativePath: string, open: boolean) {
    const runtime = this.runtime(projectId);
    const expanded = new Set(runtime.snapshot.expanded);
    if (!open) {
      expanded.delete(relativePath);
      this.setSnapshot(runtime, { ...runtime.snapshot, expanded });
      return;
    }
    expanded.add(relativePath);
    this.setSnapshot(runtime, { ...runtime.snapshot, expanded });
    let current = relativePath;
    for (let depth = 0; depth < FileExplorerStore.DIRECTORY_CHAIN_EXPANSION_LIMIT; depth += 1) {
      await this.loadDirectory(projectId, current);
      const target = findNode(runtime.snapshot.roots, current);
      const onlyChild = target?.children?.length === 1 ? target.children[0] : null;
      if (!onlyChild || onlyChild.kind !== 'directory') return;
      current = onlyChild.relativePath;
      const nextExpanded = new Set(runtime.snapshot.expanded);
      nextExpanded.add(current);
      this.setSnapshot(runtime, { ...runtime.snapshot, expanded: nextExpanded });
    }
  }

  async loadDirectory(projectId: string, relativePath: string, force = false) {
    const runtime = this.runtime(projectId);
    const target = findNode(runtime.snapshot.roots, relativePath);
    if (!target || target.kind !== 'directory') return;
    if (!force && (target.loading || target.children !== null)) return;
    const request = (runtime.directoryRequests.get(relativePath) ?? 0) + 1;
    runtime.directoryRequests.set(relativePath, request);
    if (target.children === null) {
      this.setSnapshot(runtime, {
        ...runtime.snapshot,
        roots: updateNode(runtime.snapshot.roots, relativePath, (node) => ({ ...node, loading: true })),
      });
    }
    try {
      const entries = await listWorkspaceDirectory(projectId, relativePath);
      if (runtime.directoryRequests.get(relativePath) !== request) return;
      const current = findNode(runtime.snapshot.roots, relativePath);
      if (!current || current.kind !== 'directory') return;
      const children = mergeDirectoryNodes(current.children, entries);
      if (children === current.children && !current.loading) return;
      this.setSnapshot(runtime, {
        ...runtime.snapshot,
        roots: updateNode(runtime.snapshot.roots, relativePath, (node) => ({ ...node, children, loading: false })),
      });
    } catch (reason) {
      if (runtime.directoryRequests.get(relativePath) !== request) return;
      this.setSnapshot(runtime, {
        ...runtime.snapshot,
        roots: updateNode(runtime.snapshot.roots, relativePath, (node) => ({ ...node, loading: false })),
        errorCode: commandErrorCode(reason, 'workspace-file.read-failed'),
      });
    }
  }

  setSearchQuery(projectId: string, query: string) {
    const runtime = this.runtime(projectId);
    if (runtime.searchTimer) clearTimeout(runtime.searchTimer);
    const trimmed = query.trim();
    runtime.searchRevision += 1;
    this.setSnapshot(runtime, {
      ...runtime.snapshot,
      searchQuery: query,
      searchStatus: trimmed ? 'loading' : 'idle',
      searchResult: trimmed ? runtime.snapshot.searchResult : null,
    });
    if (!trimmed) return;
    const revision = runtime.searchRevision;
    runtime.searchTimer = setTimeout(() => void this.runSearch(runtime, trimmed, revision), this.config.searchDebounceMs);
  }

  async revealFile(projectId: string, relativePath: string) {
    const runtime = this.runtime(projectId);
    if (runtime.searchTimer) {
      clearTimeout(runtime.searchTimer);
      runtime.searchTimer = null;
    }
    runtime.searchRevision += 1;
    this.setSnapshot(runtime, {
      ...runtime.snapshot,
      searchQuery: '',
      searchStatus: 'idle',
      searchResult: null,
    });
    const segments = relativePath.replaceAll('\\', '/').split('/').slice(0, -1);
    let current = '';
    for (const segment of segments) {
      current = current ? `${current}/${segment}` : segment;
      const expanded = new Set(runtime.snapshot.expanded);
      expanded.add(current);
      this.setSnapshot(runtime, { ...runtime.snapshot, expanded });
      await this.loadDirectory(projectId, current);
    }
  }

  clear(projectId: string) {
    const runtime = this.projects.get(projectId);
    if (runtime?.searchTimer) clearTimeout(runtime.searchTimer);
    if (runtime?.refreshTimer) clearTimeout(runtime.refreshTimer);
    this.projects.delete(projectId);
    this.emit();
  }

  invalidate(projectId: string, canonicalPath?: string) {
    const runtime = this.runtime(projectId);
    const parent = canonicalPath ? relativeParentFor(runtime.snapshot, canonicalPath) : null;
    if (!parent) {
      runtime.refreshAll = true;
      runtime.pendingRefreshDirectories.clear();
    } else if (!runtime.refreshAll) {
      runtime.pendingRefreshDirectories.add(parent);
      if (runtime.pendingRefreshDirectories.size > FileExplorerStore.MAX_PENDING_REFRESH_DIRECTORIES) {
        runtime.refreshAll = true;
        runtime.pendingRefreshDirectories.clear();
      }
    }
    runtime.refreshStartedAt ??= Date.now();
    if (runtime.refreshPromise) {
      runtime.refreshDirty = true;
      return;
    }
    this.armRefresh(projectId);
  }

  private armRefresh(projectId: string) {
    const runtime = this.runtime(projectId);
    if (runtime.refreshTimer) clearTimeout(runtime.refreshTimer);
    const elapsed = Date.now() - (runtime.refreshStartedAt ?? Date.now());
    const delay = Math.min(
      this.config.watchDebounceMs,
      Math.max(0, FileExplorerStore.MAX_REFRESH_LATENCY_MS - elapsed),
    );
    runtime.refreshTimer = setTimeout(() => {
      runtime.refreshTimer = null;
      runtime.refreshStartedAt = null;
      void this.runRefresh(projectId);
    }, delay);
  }

  applyFileChange(event: WorkspaceFileChangedEventVm) {
    if (!fileChangeAffectsTree(this.runtime(event.projectId, false).snapshot, event)) return;
    this.invalidate(event.projectId, event.canonicalPath);
  }

  private refreshActiveSearch(runtime: ProjectRuntime) {
    const query = runtime.snapshot.searchQuery.trim();
    if (!query) return Promise.resolve();
    runtime.searchRevision += 1;
    return this.runSearch(runtime, query, runtime.searchRevision, true);
  }

  private async runSearch(runtime: ProjectRuntime, query: string, revision: number, background = false) {
    const requestId = `${runtime.snapshot.projectId}:${revision}`;
    try {
      const result = await searchWorkspaceFiles(runtime.snapshot.projectId, query, requestId, this.config.searchResultLimit);
      if (runtime.searchRevision !== revision || result.requestId !== requestId) return;
      this.setSnapshot(runtime, { ...runtime.snapshot, searchStatus: 'ready', searchResult: result });
    } catch (reason) {
      if (runtime.searchRevision !== revision) return;
      if (background && runtime.snapshot.searchResult) return;
      this.setSnapshot(runtime, {
        ...runtime.snapshot,
        searchStatus: 'error',
        errorCode: commandErrorCode(reason, 'workspace-file.search-failed'),
      });
    }
  }

  private async refreshDirectory(projectId: string, relativePath: string) {
    const runtime = this.runtime(projectId);
    await this.loadDirectory(projectId, relativePath, true);
    const descendants = [...runtime.snapshot.expanded]
      .filter((path) => path.startsWith(`${relativePath}/`))
      .sort((left, right) => pathDepth(left) - pathDepth(right));
    for (const path of descendants) await this.loadDirectory(projectId, path, true);
  }

  private async runRefresh(projectId: string, forceAll = false) {
    const runtime = this.runtime(projectId);
    if (runtime.refreshPromise) {
      runtime.refreshDirty = true;
      runtime.refreshAll ||= forceAll;
      return runtime.refreshPromise;
    }
    if (runtime.refreshTimer) clearTimeout(runtime.refreshTimer);
    runtime.refreshTimer = null;
    runtime.refreshStartedAt = null;
    const refreshAll = forceAll || runtime.refreshAll || runtime.pendingRefreshDirectories.size === 0;
    const directories = refreshAll
      ? []
      : minimalDirectorySet(runtime.pendingRefreshDirectories);
    runtime.refreshAll = false;
    runtime.pendingRefreshDirectories.clear();
    const request = Promise.all([
      refreshAll
        ? this.refreshRoot(projectId)
        : directories.reduce(
            (previous, directory) => previous.then(() => this.refreshDirectory(projectId, directory)),
            Promise.resolve(),
          ),
      this.refreshActiveSearch(runtime),
    ]).then(() => undefined).finally(() => {
      if (runtime.refreshPromise === request) runtime.refreshPromise = null;
      if (runtime.refreshDirty || runtime.refreshAll || runtime.pendingRefreshDirectories.size > 0) {
        runtime.refreshDirty = false;
        runtime.refreshStartedAt ??= Date.now();
        this.armRefresh(projectId);
      }
    });
    runtime.refreshPromise = request;
    return request;
  }

  private runtime(projectId: string, touch = true) {
    let runtime = this.projects.get(projectId);
    if (!runtime) {
      runtime = {
        snapshot: idleSnapshot(projectId),
        operations: [],
        revealedSelectionPath: null,
        directoryRequests: new Map(),
        searchRevision: 0,
        searchTimer: null,
        refreshTimer: null,
        refreshStartedAt: null,
        refreshPromise: null,
        refreshDirty: false,
        refreshAll: false,
        pendingRefreshDirectories: new Set(),
      };
      this.projects.set(projectId, runtime);
      while (this.projects.size > FileExplorerStore.MAX_PROJECTS) {
        const oldest = this.projects.keys().next().value as string | undefined;
        if (!oldest || oldest === projectId) break;
        const evicted = this.projects.get(oldest);
        if (evicted?.searchTimer) clearTimeout(evicted.searchTimer);
        if (evicted?.refreshTimer) clearTimeout(evicted.refreshTimer);
        this.projects.delete(oldest);
      }
    } else if (touch) {
      this.projects.delete(projectId);
      this.projects.set(projectId, runtime);
    }
    return runtime;
  }

  private setSnapshot(runtime: ProjectRuntime, snapshot: FileExplorerSnapshot) {
    runtime.snapshot = snapshot;
    this.emit();
  }

  private emit() {
    for (const listener of this.listeners) listener();
  }
}

function pathDepth(path: string) {
  return path.split(/[\\/]/u).length;
}

function minimalDirectorySet(paths: ReadonlySet<string>) {
  const sorted = [...paths].sort((left, right) => pathDepth(left) - pathDepth(right));
  return sorted.filter((path, index) => !sorted.slice(0, index).some((parent) => path.startsWith(`${parent}/`)));
}

function relativeParentFor(snapshot: FileExplorerSnapshot, canonicalPath: string) {
  const normalizedCanonical = normalizePath(canonicalPath);
  const rootEntry = snapshot.roots[0];
  if (!rootEntry) return null;
  const normalizedEntry = normalizePath(rootEntry.canonicalPath);
  const relativeEntry = normalizePath(rootEntry.relativePath);
  const rootLength = normalizedEntry.length - relativeEntry.length;
  if (rootLength < 0) return null;
  const root = normalizedEntry.slice(0, rootLength).replace(/\/$/u, '');
  if (normalizedCanonical !== root && !normalizedCanonical.startsWith(`${root}/`)) return null;
  const relative = normalizedCanonical.slice(root.length).replace(/^\//u, '');
  const parent = relative.split('/').slice(0, -1).join('/');
  return parent || null;
}

function normalizePath(path: string) {
  const normalized = path.replaceAll('\\', '/');
  return /^[a-z]:\//iu.test(normalized) ? normalized.toLowerCase() : normalized;
}

export const fileExplorerStore = new FileExplorerStore();

export function useFileExplorerSnapshot(projectId: string) {
  return useSyncExternalStore(
    fileExplorerStore.subscribe,
    () => fileExplorerStore.snapshot(projectId),
    () => fileExplorerStore.snapshot(projectId),
  );
}
