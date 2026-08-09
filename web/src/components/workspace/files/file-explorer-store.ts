import { useSyncExternalStore } from 'react';
import { listWorkspaceDirectory, searchWorkspaceFiles } from '@/api';
import type { WorkspaceDirectoryEntryVm, WorkspaceFileChangedEventVm, WorkspaceFileSearchVm, WorkspaceFilesVm } from '@/types';
import { FALLBACK_WORKSPACE_FILES } from '../workspace-layout';

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
}

interface ProjectRuntime {
  snapshot: FileExplorerSnapshot;
  revealedSelectionPath: string | null;
  directoryRequests: Map<string, number>;
  searchRevision: number;
  searchTimer: ReturnType<typeof setTimeout> | null;
  refreshTimer: ReturnType<typeof setTimeout> | null;
}

function commandErrorCode(reason: unknown, fallback: string) {
  return typeof reason === 'object' && reason && 'code' in reason && typeof reason.code === 'string'
    ? reason.code
    : fallback;
}

function nodesFor(entries: WorkspaceDirectoryEntryVm[]): FileTreeNode[] {
  return entries.map((entry) => ({
    ...entry,
    id: entry.relativePath,
    children: entry.kind === 'directory' ? null : [],
    loading: false,
  }));
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
  };
}

export class FileExplorerStore {
  private static readonly MAX_PROJECTS = 24;
  private static readonly DIRECTORY_CHAIN_EXPANSION_LIMIT = 64;
  private config: WorkspaceFilesVm = FALLBACK_WORKSPACE_FILES;
  private readonly projects = new Map<string, ProjectRuntime>();
  private readonly listeners = new Set<() => void>();

  configure(config: WorkspaceFilesVm) {
    this.config = config;
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

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

  private async refreshRoot(projectId: string) {
    const runtime = this.runtime(projectId);
    if (runtime.snapshot.status !== 'ready') {
      await this.loadRoot(projectId, true);
      return;
    }
    try {
      const entries = await listWorkspaceDirectory(projectId, '');
      this.setSnapshot(runtime, { ...runtime.snapshot, roots: nodesFor(entries), errorCode: null });
      const expanded = [...runtime.snapshot.expanded].sort((left, right) => pathDepth(left) - pathDepth(right));
      for (const id of expanded) await this.loadDirectory(projectId, id);
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
    this.setSnapshot(runtime, {
      ...runtime.snapshot,
      roots: updateNode(runtime.snapshot.roots, relativePath, (node) => ({ ...node, loading: true })),
    });
    try {
      const entries = await listWorkspaceDirectory(projectId, relativePath);
      if (runtime.directoryRequests.get(relativePath) !== request) return;
      this.setSnapshot(runtime, {
        ...runtime.snapshot,
        roots: updateNode(runtime.snapshot.roots, relativePath, (node) => ({ ...node, children: nodesFor(entries), loading: false })),
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
    if (runtime.refreshTimer) clearTimeout(runtime.refreshTimer);
    runtime.refreshTimer = setTimeout(() => {
      runtime.refreshTimer = null;
      const parent = canonicalPath ? relativeParentFor(runtime.snapshot, canonicalPath) : null;
      if (parent) void this.refreshDirectory(projectId, parent);
      else void this.refreshRoot(projectId);
    }, this.config.watchDebounceMs);
  }

  applyFileChange(event: WorkspaceFileChangedEventVm) {
    if (!fileChangeAffectsTree(this.runtime(event.projectId, false).snapshot, event)) return;
    this.invalidate(event.projectId, event.canonicalPath);
  }

  private async runSearch(runtime: ProjectRuntime, query: string, revision: number) {
    const requestId = `${runtime.snapshot.projectId}:${revision}`;
    try {
      const result = await searchWorkspaceFiles(runtime.snapshot.projectId, query, requestId, this.config.searchResultLimit);
      if (runtime.searchRevision !== revision || result.requestId !== requestId) return;
      this.setSnapshot(runtime, { ...runtime.snapshot, searchStatus: 'ready', searchResult: result });
    } catch (reason) {
      if (runtime.searchRevision !== revision) return;
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
    for (const path of descendants) await this.loadDirectory(projectId, path);
  }

  private runtime(projectId: string, touch = true) {
    let runtime = this.projects.get(projectId);
    if (!runtime) {
      runtime = {
        snapshot: idleSnapshot(projectId),
        revealedSelectionPath: null,
        directoryRequests: new Map(),
        searchRevision: 0,
        searchTimer: null,
        refreshTimer: null,
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
