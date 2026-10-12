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
  WorkspaceRootRef,
} from '@/types';
import { workspaceRootKey } from '@/lib/workspace-root';
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
  | { root: WorkspaceRootRef; kind: 'removed'; entry: WorkspaceDirectoryEntryVm }
  | { root: WorkspaceRootRef; kind: 'moved'; from: WorkspaceDirectoryEntryVm; to: WorkspaceDirectoryEntryVm };

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
  root: WorkspaceRootRef;
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

interface RootRuntime {
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
  rootListing: Promise<void> | null;
  rootListingStale: boolean;
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

/** Whether the directory that would list this path is loaded; paths under unloaded directories are not visible. */
function parentDirectoryLoaded(snapshot: FileExplorerSnapshot, canonicalPath: string) {
  // An empty root has no entry to anchor the path against, so relist the root.
  if (!snapshot.roots.length) return true;
  const parent = relativeParentFor(snapshot, canonicalPath);
  if (parent === null) return false;
  return parent === '' || findNode(snapshot.roots, parent)?.children != null;
}

export function fileChangeAffectsTree(
  snapshot: FileExplorerSnapshot,
  event: WorkspaceFileChangedEventVm,
): boolean {
  // Mutations issued by this app refresh their own parents.
  if (event.operationId) return false;
  // A listing in flight may predate this change.
  if (snapshot.status !== 'ready') return snapshot.status === 'loading';
  // Watcher kinds depend on the platform and on batching (a checked-out file
  // can arrive as `modified`), so the loaded tree decides what changed shape;
  // the kind only tells whether the path still exists. A known path that still
  // exists is a content change, including an atomic replacement.
  const knownPath = containsCanonicalPath(snapshot.roots, event.canonicalPath);
  const exists = event.kind !== 'removed' && !(event.kind === 'renamed' && !event.revision);
  if (!exists) return knownPath;
  return !knownPath && parentDirectoryLoaded(snapshot, event.canonicalPath);
}

function idleSnapshot(root: WorkspaceRootRef): FileExplorerSnapshot {
  return {
    root,
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
  private static readonly MAX_ROOTS = 24;
  /** Undo depth per root; history is session-only like the backend trash receipts. */
  private static readonly MAX_UNDO_OPERATIONS = 20;
  private static readonly DIRECTORY_CHAIN_EXPANSION_LIMIT = 64;
  private static readonly MAX_REFRESH_LATENCY_MS = 1_000;
  private static readonly MAX_PENDING_REFRESH_DIRECTORIES = 64;
  private config: WorkspaceFilesVm = FALLBACK_WORKSPACE_FILES;
  private readonly roots = new Map<string, RootRuntime>();
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

  canUndo(root: WorkspaceRootRef) {
    return (this.roots.get(rootKeyOf(root))?.operations.length ?? 0) > 0;
  }

  /** Show an inline name editor for a new entry inside `parentRelativePath` ('' is the root). */
  async startDraft(root: WorkspaceRootRef, parentRelativePath: string, kind: WorkspaceEntryKind) {
    const runtime = this.runtime(root);
    if (runtime.snapshot.pendingPath !== null) return;
    if (parentRelativePath && !runtime.snapshot.expanded.has(parentRelativePath)) {
      await this.toggleDirectory(root, parentRelativePath, true);
    } else if (parentRelativePath) {
      await this.loadDirectory(root, parentRelativePath);
    }
    this.setSnapshot(runtime, { ...runtime.snapshot, draft: { parentRelativePath, kind } });
  }

  cancelDraft(root: WorkspaceRootRef) {
    const runtime = this.runtime(root);
    if (!runtime.snapshot.draft) return;
    this.setSnapshot(runtime, { ...runtime.snapshot, draft: null });
  }

  async createEntry(root: WorkspaceRootRef, name: string): Promise<FileTreeOperationResult> {
    const runtime = this.runtime(root);
    const draft = runtime.snapshot.draft;
    if (!draft || runtime.snapshot.pendingPath !== null) return { status: 'skipped' };
    return this.runOperation(runtime, draft.parentRelativePath, async () => {
      try {
        const entry = await createWorkspaceEntry({
          ...root,
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

  async renameEntry(root: WorkspaceRootRef, entry: WorkspaceDirectoryEntryVm, newName: string): Promise<FileTreeOperationResult> {
    const runtime = this.runtime(root);
    if (newName === entry.name || runtime.snapshot.pendingPath !== null) return { status: 'skipped' };
    return this.runOperation(runtime, entry.relativePath, async () => {
      const to = await this.renameWithContent(root, entry, newName);
      this.pushOperation(runtime, { kind: 'rename', from: entry, to });
      return { status: 'done', entry: to };
    }, [parentRelativePath(entry.relativePath)]);
  }

  async deleteEntry(root: WorkspaceRootRef, entry: WorkspaceDirectoryEntryVm): Promise<FileTreeOperationResult> {
    const runtime = this.runtime(root);
    if (runtime.snapshot.pendingPath !== null) return { status: 'skipped' };
    return this.runOperation(runtime, entry.relativePath, async () => {
      const deletion = await deleteWorkspaceEntry(root, entry.relativePath);
      this.afterRemoved(runtime, deletion.entry);
      this.pushOperation(runtime, { kind: 'delete', receiptId: deletion.receiptId, entry: deletion.entry });
      return { status: 'done', entry: null };
    }, [parentRelativePath(entry.relativePath)]);
  }

  /** Invert the most recent operation. A failed undo is dropped, like a failed redo target in editors. */
  async undo(root: WorkspaceRootRef): Promise<FileTreeOperationResult> {
    const runtime = this.runtime(root);
    const operation = runtime.operations.at(-1);
    if (!operation || runtime.snapshot.pendingPath !== null) return { status: 'skipped' };
    runtime.operations.pop();
    switch (operation.kind) {
      case 'create':
        return this.runOperation(runtime, operation.entry.relativePath, async () => {
          const deletion = await deleteWorkspaceEntry(root, operation.entry.relativePath);
          this.afterRemoved(runtime, deletion.entry);
          return { status: 'done', entry: null };
        }, [parentRelativePath(operation.entry.relativePath)]);
      case 'rename':
        return this.runOperation(runtime, operation.to.relativePath, async () => {
          const entry = await this.renameWithContent(root, operation.to, operation.from.name);
          return { status: 'done', entry };
        }, [parentRelativePath(operation.to.relativePath)]);
      case 'delete':
        return this.runOperation(runtime, operation.entry.relativePath, async () => {
          const entry = await restoreWorkspaceEntry(root, operation.receiptId);
          return { status: 'done', entry };
        }, [parentRelativePath(operation.entry.relativePath)]);
    }
  }

  private async renameWithContent(root: WorkspaceRootRef, entry: WorkspaceDirectoryEntryVm, newName: string) {
    // Pending autosaves must land on the old path before it disappears.
    if (!await fileContentStore.flushWithin(root.projectId, entry.canonicalPath)) {
      throw { code: 'workspace-file.unsaved-changes', params: { path: entry.canonicalPath } };
    }
    const to = await renameWorkspaceEntry(root, entry.relativePath, newName);
    const runtime = this.runtime(root);
    const expanded = new Set([...runtime.snapshot.expanded].map((path) => remapRelativePath(path, entry.relativePath, to.relativePath)));
    this.setSnapshot(runtime, { ...runtime.snapshot, expanded });
    await fileContentStore.releaseWithin(root.projectId, entry.canonicalPath);
    this.emitMutation({ root, kind: 'moved', from: entry, to });
    return to;
  }

  private afterRemoved(runtime: RootRuntime, entry: WorkspaceDirectoryEntryVm) {
    const expanded = new Set([...runtime.snapshot.expanded].filter((path) => remapRelativePath(path, entry.relativePath, '') === path));
    this.setSnapshot(runtime, { ...runtime.snapshot, expanded });
    void fileContentStore.releaseWithin(runtime.snapshot.root.projectId, entry.canonicalPath);
    this.emitMutation({ root: runtime.snapshot.root, kind: 'removed', entry });
  }

  private pushOperation(runtime: RootRuntime, operation: FileTreeOperation) {
    runtime.operations.push(operation);
    if (runtime.operations.length > FileExplorerStore.MAX_UNDO_OPERATIONS) runtime.operations.shift();
  }

  private async runOperation(
    runtime: RootRuntime,
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
      await this.refreshDirectories(runtime.snapshot.root, refreshDirectories);
    }
  }

  /** Refresh mutated parents now instead of waiting for the debounced watcher event. */
  private refreshDirectories(root: WorkspaceRootRef, directories: string[]) {
    const runtime = this.runtime(root);
    if (directories.some((directory) => !directory)) return this.runRefresh(root, true);
    for (const directory of directories) runtime.pendingRefreshDirectories.add(directory);
    return this.runRefresh(root);
  }

  private emitMutation(mutation: FileTreeEntryMutation) {
    for (const listener of this.mutationListeners) listener(mutation);
  }

  snapshot = (root: WorkspaceRootRef) => this.runtime(root, false).snapshot;

  setTreeScrollTop(root: WorkspaceRootRef, treeScrollTop: number) {
    const runtime = this.runtime(root);
    const next = Math.max(0, Math.round(treeScrollTop));
    if (runtime.snapshot.treeScrollTop === next) return;
    runtime.snapshot = { ...runtime.snapshot, treeScrollTop: next };
  }

  setTreeWidth(root: WorkspaceRootRef, treeWidth: number) {
    const runtime = this.runtime(root);
    const next = Math.max(1, Math.round(treeWidth));
    if (runtime.snapshot.treeWidth === next) return;
    runtime.snapshot = { ...runtime.snapshot, treeWidth: next };
  }

  setDisplayMode(root: WorkspaceRootRef, displayMode: FileTreeDisplayMode) {
    const runtime = this.runtime(root);
    if (runtime.snapshot.displayMode === displayMode) return;
    this.setSnapshot(runtime, { ...runtime.snapshot, displayMode });
  }

  takeSelectionReveal(root: WorkspaceRootRef, canonicalPath: string | null) {
    const runtime = this.runtime(root);
    const next = canonicalPath ? normalizePath(canonicalPath) : null;
    if (runtime.revealedSelectionPath === next) return false;
    runtime.revealedSelectionPath = next;
    return next !== null;
  }

  /**
   * One root listing per root at a time. A forced request made while it is in
   * flight may observe newer state than the listing, so the listing repeats
   * before the tree becomes ready instead of starting a competing request.
   */
  loadRoot(root: WorkspaceRootRef, force = false): Promise<void> {
    const runtime = this.runtime(root);
    if (runtime.rootListing) {
      runtime.rootListingStale ||= force;
      return runtime.rootListing;
    }
    if (!force && runtime.snapshot.status === 'ready') return Promise.resolve();
    const listing = this.listRoot(root, runtime).finally(() => {
      runtime.rootListing = null;
    });
    runtime.rootListing = listing;
    return listing;
  }

  private async listRoot(root: WorkspaceRootRef, runtime: RootRuntime) {
    this.setSnapshot(runtime, { ...runtime.snapshot, status: 'loading', errorCode: null });
    try {
      let entries: WorkspaceDirectoryEntryVm[];
      do {
        runtime.rootListingStale = false;
        entries = await listWorkspaceDirectory(root, '');
      } while (runtime.rootListingStale);
      this.setSnapshot(runtime, { ...runtime.snapshot, status: 'ready', roots: nodesFor(entries), errorCode: null });
      const expanded = [...runtime.snapshot.expanded].sort((left, right) => pathDepth(left) - pathDepth(right));
      for (const id of expanded) await this.loadDirectory(root, id);
    } catch (reason) {
      this.setSnapshot(runtime, {
        ...runtime.snapshot,
        status: 'error',
        errorCode: commandErrorCode(reason, 'workspace-file.read-failed'),
      });
    }
  }

  reconcile(root: WorkspaceRootRef) {
    const runtime = this.runtime(root);
    if (runtime.snapshot.status !== 'ready') return this.loadRoot(root, true);
    return this.runRefresh(root, true);
  }

  private async refreshRoot(root: WorkspaceRootRef) {
    const runtime = this.runtime(root);
    if (runtime.snapshot.status !== 'ready') {
      await this.loadRoot(root, true);
      return;
    }
    try {
      const entries = await listWorkspaceDirectory(root, '');
      const roots = mergeDirectoryNodes(runtime.snapshot.roots, entries);
      if (roots !== runtime.snapshot.roots || runtime.snapshot.errorCode !== null) {
        this.setSnapshot(runtime, { ...runtime.snapshot, roots, errorCode: null });
      }
      const expanded = [...runtime.snapshot.expanded].sort((left, right) => pathDepth(left) - pathDepth(right));
      for (const id of expanded) await this.loadDirectory(root, id, true);
    } catch (reason) {
      this.setSnapshot(runtime, {
        ...runtime.snapshot,
        errorCode: commandErrorCode(reason, 'workspace-file.read-failed'),
      });
    }
  }

  async toggleDirectory(root: WorkspaceRootRef, relativePath: string, open: boolean) {
    const runtime = this.runtime(root);
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
      await this.loadDirectory(root, current);
      const target = findNode(runtime.snapshot.roots, current);
      const onlyChild = target?.children?.length === 1 ? target.children[0] : null;
      if (!onlyChild || onlyChild.kind !== 'directory') return;
      current = onlyChild.relativePath;
      const nextExpanded = new Set(runtime.snapshot.expanded);
      nextExpanded.add(current);
      this.setSnapshot(runtime, { ...runtime.snapshot, expanded: nextExpanded });
    }
  }

  async loadDirectory(root: WorkspaceRootRef, relativePath: string, force = false) {
    const runtime = this.runtime(root);
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
      const entries = await listWorkspaceDirectory(root, relativePath);
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

  setSearchQuery(root: WorkspaceRootRef, query: string) {
    const runtime = this.runtime(root);
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

  async revealFile(root: WorkspaceRootRef, relativePath: string) {
    const runtime = this.runtime(root);
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
      await this.loadDirectory(root, current);
    }
  }

  /** Drop every root of a removed project. */
  clearProject(projectId: string) {
    for (const [key, runtime] of this.roots) {
      if (runtime.snapshot.root.projectId !== projectId) continue;
      if (runtime.searchTimer) clearTimeout(runtime.searchTimer);
      if (runtime.refreshTimer) clearTimeout(runtime.refreshTimer);
      this.roots.delete(key);
    }
    this.emit();
  }

  invalidate(root: WorkspaceRootRef, canonicalPath?: string) {
    const runtime = this.runtime(root);
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
    this.armRefresh(root);
  }

  private armRefresh(root: WorkspaceRootRef) {
    const runtime = this.runtime(root);
    if (runtime.refreshTimer) clearTimeout(runtime.refreshTimer);
    const elapsed = Date.now() - (runtime.refreshStartedAt ?? Date.now());
    const delay = Math.min(
      this.config.watchDebounceMs,
      Math.max(0, FileExplorerStore.MAX_REFRESH_LATENCY_MS - elapsed),
    );
    runtime.refreshTimer = setTimeout(() => {
      runtime.refreshTimer = null;
      runtime.refreshStartedAt = null;
      void this.runRefresh(root);
    }, delay);
  }

  /** Route a watch event to the explorer of the exact root it was emitted for. */
  applyFileChange(event: WorkspaceFileChangedEventVm) {
    const runtime = this.roots.get(workspaceRootKey(event.projectId, event.workspacePath));
    if (!runtime || !fileChangeAffectsTree(runtime.snapshot, event)) return;
    this.invalidate(runtime.snapshot.root, event.canonicalPath);
  }

  private refreshActiveSearch(runtime: RootRuntime) {
    const query = runtime.snapshot.searchQuery.trim();
    if (!query) return Promise.resolve();
    runtime.searchRevision += 1;
    return this.runSearch(runtime, query, runtime.searchRevision, true);
  }

  private async runSearch(runtime: RootRuntime, query: string, revision: number, background = false) {
    const requestId = `${rootKeyOf(runtime.snapshot.root)}:${revision}`;
    try {
      const result = await searchWorkspaceFiles(runtime.snapshot.root, query, requestId, this.config.searchResultLimit);
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

  private async refreshDirectory(root: WorkspaceRootRef, relativePath: string) {
    const runtime = this.runtime(root);
    await this.loadDirectory(root, relativePath, true);
    const descendants = [...runtime.snapshot.expanded]
      .filter((path) => path.startsWith(`${relativePath}/`))
      .sort((left, right) => pathDepth(left) - pathDepth(right));
    for (const path of descendants) await this.loadDirectory(root, path, true);
  }

  private async runRefresh(root: WorkspaceRootRef, forceAll = false) {
    const runtime = this.runtime(root);
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
        ? this.refreshRoot(root)
        : directories.reduce(
            (previous, directory) => previous.then(() => this.refreshDirectory(root, directory)),
            Promise.resolve(),
          ),
      this.refreshActiveSearch(runtime),
    ]).then(() => undefined).finally(() => {
      if (runtime.refreshPromise === request) runtime.refreshPromise = null;
      if (runtime.refreshDirty || runtime.refreshAll || runtime.pendingRefreshDirectories.size > 0) {
        runtime.refreshDirty = false;
        runtime.refreshStartedAt ??= Date.now();
        this.armRefresh(root);
      }
    });
    runtime.refreshPromise = request;
    return request;
  }

  private runtime(root: WorkspaceRootRef, touch = true) {
    const key = rootKeyOf(root);
    let runtime = this.roots.get(key);
    if (!runtime) {
      runtime = {
        snapshot: idleSnapshot(root),
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
        rootListing: null,
        rootListingStale: false,
      };
      this.roots.set(key, runtime);
      while (this.roots.size > FileExplorerStore.MAX_ROOTS) {
        const oldest = this.roots.keys().next().value as string | undefined;
        if (!oldest || oldest === key) break;
        const evicted = this.roots.get(oldest);
        if (evicted?.searchTimer) clearTimeout(evicted.searchTimer);
        if (evicted?.refreshTimer) clearTimeout(evicted.refreshTimer);
        this.roots.delete(oldest);
      }
    } else if (touch) {
      this.roots.delete(key);
      this.roots.set(key, runtime);
    }
    return runtime;
  }

  private setSnapshot(runtime: RootRuntime, snapshot: FileExplorerSnapshot) {
    runtime.snapshot = snapshot;
    this.emit();
  }

  private emit() {
    for (const listener of this.listeners) listener();
  }
}

function rootKeyOf(root: WorkspaceRootRef) {
  return workspaceRootKey(root.projectId, root.workspacePath);
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
  return relative.split('/').slice(0, -1).join('/');
}

function normalizePath(path: string) {
  const normalized = path.replaceAll('\\', '/');
  return /^[a-z]:\//iu.test(normalized) ? normalized.toLowerCase() : normalized;
}

export const fileExplorerStore = new FileExplorerStore();

export function useFileExplorerSnapshot(root: WorkspaceRootRef) {
  return useSyncExternalStore(
    fileExplorerStore.subscribe,
    () => fileExplorerStore.snapshot(root),
    () => fileExplorerStore.snapshot(root),
  );
}
