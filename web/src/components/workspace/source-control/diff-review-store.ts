import { getGitComparison } from '@/api';
import type { GitComparisonSourceVm, GitFileChangeVm, GitFileComparisonVm } from '@/types';
import { normalizeSourceControlWorkspacePath } from './source-control-identity';

export interface GitDiffReviewItem {
  id: string;
  path: string;
  source: GitComparisonSourceVm;
  stats: { addedLines: number | null; deletedLines: number | null };
}

export interface WorkspaceReviewScope {
  workspacePath: string | null;
  area: 'staged' | 'unstaged';
}

export interface GitDiffReviewSession {
  id: string;
  projectId: string;
  revision: string;
  items: GitDiffReviewItem[];
  workspace?: WorkspaceReviewScope;
}

export interface WorkspaceReviewRefresh {
  projectId: string;
  workspacePath: string | null | undefined;
  staged: WorkspaceReviewChange[];
  unstaged: WorkspaceReviewChange[];
  untracked: WorkspaceReviewChange[];
  invalidate: { all: boolean; paths: readonly string[] };
}

type WorkspaceReviewChange = Pick<GitFileChangeVm, 'path' | 'addedLines' | 'deletedLines'>;

const SESSION_LIMIT = 12;
const COMPARISON_LIMIT = 48;

class DiffReviewStore {
  private readonly sessions = new Map<string, GitDiffReviewSession>();
  private readonly comparisons = new Map<string, Promise<GitFileComparisonVm>>();
  private readonly workspaceEpochs = new Map<string, number>();
  private readonly pathEpochs = new Map<string, number>();
  private readonly listeners = new Set<() => void>();
  private revision = 0;

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  version = () => this.revision;

  save(session: GitDiffReviewSession) {
    this.sessions.delete(session.id);
    this.sessions.set(session.id, session);
    while (this.sessions.size > SESSION_LIMIT) {
      const oldest = this.sessions.keys().next().value as string | undefined;
      if (!oldest) break;
      this.sessions.delete(oldest);
    }
  }

  get(sessionId: string) {
    const session = this.sessions.get(sessionId);
    if (!session) return null;
    this.sessions.delete(sessionId);
    this.sessions.set(sessionId, session);
    return session;
  }

  workspaceContentEpoch(projectId: string, source: GitComparisonSourceVm) {
    if (source.kind !== 'workspace') return 0;
    return (this.workspaceEpochs.get(this.workspaceKey(projectId, source.workspacePath)) ?? 0)
      + (this.pathEpochs.get(this.pathKey(projectId, source)) ?? 0);
  }

  publishWorkspaceRefresh(input: WorkspaceReviewRefresh) {
    const synced = this.syncWorkspaceReviews(input);
    const invalidated = input.invalidate.all
      ? this.bumpWorkspace(input.projectId, input.workspacePath)
      : input.invalidate.paths.reduce((changed, path) => (
        this.bumpUnstagedPath(input.projectId, input.workspacePath, path) || changed
      ), false);
    if (synced || invalidated) this.emit();
  }

  clearForTests() {
    this.sessions.clear();
    this.comparisons.clear();
    this.workspaceEpochs.clear();
    this.pathEpochs.clear();
    this.revision += 1;
  }

  comparison(projectId: string, item: GitDiffReviewItem) {
    const key = this.comparisonCacheKey(projectId, item);
    const cached = this.comparisons.get(key);
    if (cached) {
      this.comparisons.delete(key);
      this.comparisons.set(key, cached);
      return cached;
    }
    const request = getGitComparison(projectId, item.source)
      .then((comparison) => ({
        ...comparison,
        stats: resolveReviewComparisonStats(item.stats, comparison.stats),
      }))
      .catch((error) => {
        this.comparisons.delete(key);
        throw error;
      });
    this.comparisons.set(key, request);
    while (this.comparisons.size > COMPARISON_LIMIT) {
      const oldest = this.comparisons.keys().next().value as string | undefined;
      if (!oldest) break;
      this.comparisons.delete(oldest);
    }
    return request;
  }

  prefetchAdjacent(sessionId: string, activeItemId: string) {
    const session = this.get(sessionId);
    if (!session) return;
    const index = session.items.findIndex((item) => item.id === activeItemId);
    for (const adjacent of [session.items[index - 1], session.items[index + 1]]) {
      if (adjacent) void this.comparison(session.projectId, adjacent).catch(() => undefined);
    }
  }

  private comparisonCacheKey(projectId: string, item: GitDiffReviewItem) {
    if (item.source.kind === 'workspace') {
      return `workspace\0${this.pathKey(projectId, item.source)}\0${this.workspaceContentEpoch(projectId, item.source)}`;
    }
    return `immutable\0${projectId}\0${item.id}`;
  }

  private workspaceKey(projectId: string, workspacePath: string | null | undefined) {
    return `${projectId}\0${normalizeSourceControlWorkspacePath(workspacePath) ?? ''}`;
  }

  private pathKey(projectId: string, source: Extract<GitComparisonSourceVm, { kind: 'workspace' }>) {
    return `${this.workspaceKey(projectId, source.workspacePath)}\0${source.area}\0${source.path.replaceAll('\\', '/')}`;
  }

  private bumpWorkspace(projectId: string, workspacePath: string | null | undefined) {
    const key = this.workspaceKey(projectId, workspacePath);
    this.workspaceEpochs.set(key, (this.workspaceEpochs.get(key) ?? 0) + 1);
    const prefix = `workspace\0${key}\0`;
    this.dropComparisons((cacheKey) => cacheKey.startsWith(prefix));
    return true;
  }

  private bumpUnstagedPath(projectId: string, workspacePath: string | null | undefined, relativePath: string) {
    const source = {
      kind: 'workspace' as const,
      workspacePath: workspacePath ?? null,
      area: 'unstaged' as const,
      path: relativePath.replaceAll('\\', '/'),
    };
    const key = this.pathKey(projectId, source);
    this.pathEpochs.set(key, (this.pathEpochs.get(key) ?? 0) + 1);
    this.dropComparisons((cacheKey) => cacheKey.startsWith(`workspace\0${key}\0`));
    return true;
  }

  private dropComparisons(predicate: (cacheKey: string) => boolean) {
    for (const cacheKey of this.comparisons.keys()) {
      if (predicate(cacheKey)) this.comparisons.delete(cacheKey);
    }
  }

  private syncWorkspaceReviews(input: WorkspaceReviewRefresh) {
    let changed = false;
    for (const session of this.sessions.values()) {
      if (!session.workspace || session.projectId !== input.projectId) continue;
      if (normalizeSourceControlWorkspacePath(session.workspace.workspacePath)
        !== normalizeSourceControlWorkspacePath(input.workspacePath ?? null)) continue;
      const changes = session.workspace.area === 'staged'
        ? input.staged
        : [...input.unstaged, ...input.untracked];
      const items = workspaceReviewItems(session.workspace.workspacePath, session.workspace.area, changes);
      if (reviewItemsEqual(session.items, items)) continue;
      session.items = items;
      changed = true;
    }
    return changed;
  }

  private emit() {
    this.revision += 1;
    for (const listener of this.listeners) listener();
  }
}

export const diffReviewStore = new DiffReviewStore();

export function resolveReviewComparisonStats(
  authoritative: GitDiffReviewItem['stats'],
  computed: GitFileComparisonVm['stats'],
) {
  return {
    addedLines: authoritative.addedLines ?? computed.addedLines,
    deletedLines: authoritative.deletedLines ?? computed.deletedLines,
  };
}

export function gitDiffReviewItemId(afterOid: string, beforeOid: string | null | undefined, beforePath: string | null | undefined, path: string) {
  return `${afterOid}:${beforeOid ?? ''}:${beforePath ?? ''}:${path}`;
}

export function workspaceReviewItems(
  workspacePath: string | null | undefined,
  area: 'staged' | 'unstaged',
  changes: WorkspaceReviewChange[],
): GitDiffReviewItem[] {
  return changes.map((change) => {
    const source = { kind: 'workspace' as const, workspacePath, path: change.path, area };
    return {
      id: gitComparisonReviewItemId(source),
      path: change.path,
      source,
      stats: { addedLines: change.addedLines ?? null, deletedLines: change.deletedLines ?? null },
    };
  });
}

export function shouldRetainVisibleComparison(currentPath: string | null | undefined, nextPath: string | null | undefined) {
  return Boolean(currentPath && nextPath && currentPath === nextPath);
}

function reviewItemsEqual(left: GitDiffReviewItem[], right: GitDiffReviewItem[]) {
  return left.length === right.length && left.every((item, index) => {
    const other = right[index];
    return item.id === other?.id
      && item.stats.addedLines === other.stats.addedLines
      && item.stats.deletedLines === other.stats.deletedLines;
  });
}

export function gitComparisonReviewItemId(source: GitComparisonSourceVm) {
  if (source.kind === 'workspace') return `workspace:${source.workspacePath ?? ''}:${source.area}:${source.path}`;
  if (source.kind === 'commit') return `commit:${gitDiffReviewItemId(source.afterOid, source.beforeOid, source.beforePath, source.path)}`;
  return `github-pr:${source.workspacePath ?? ''}:${source.host}:${source.repository}:${source.prNumber}:${source.baseOid}:${source.headOid}:${source.beforePath ?? ''}:${source.path}`;
}

export function resolveDiffReviewNavigation(input: {
  itemIndex: number;
  itemCount: number;
  chunkIndex: number;
  chunkCount: number;
  direction: -1 | 1;
}): { kind: 'chunk'; index: number } | { kind: 'file'; offset: -1 | 1; landing: 'first' | 'last' } | { kind: 'none' } {
  const nextChunk = input.chunkIndex + input.direction;
  if (nextChunk >= 0 && nextChunk < input.chunkCount) return { kind: 'chunk', index: nextChunk };
  const nextItem = input.itemIndex + input.direction;
  if (nextItem < 0 || nextItem >= input.itemCount) return { kind: 'none' };
  return { kind: 'file', offset: input.direction, landing: input.direction < 0 ? 'last' : 'first' };
}
