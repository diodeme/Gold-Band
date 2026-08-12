import { getGitComparison } from '@/api';
import type { GitComparisonSourceVm, GitFileComparisonVm } from '@/types';

export interface GitDiffReviewItem {
  id: string;
  path: string;
  source: Extract<GitComparisonSourceVm, { kind: 'commit' }>;
}

export interface GitDiffReviewSession {
  id: string;
  projectId: string;
  revision: string;
  items: GitDiffReviewItem[];
}

const SESSION_LIMIT = 12;
const COMPARISON_LIMIT = 48;

class DiffReviewStore {
  private readonly sessions = new Map<string, GitDiffReviewSession>();
  private readonly comparisons = new Map<string, Promise<GitFileComparisonVm>>();

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

  comparison(projectId: string, item: GitDiffReviewItem) {
    const key = `${projectId}:${item.id}`;
    const cached = this.comparisons.get(key);
    if (cached) {
      this.comparisons.delete(key);
      this.comparisons.set(key, cached);
      return cached;
    }
    const request = getGitComparison(projectId, item.source).catch((error) => {
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
}

export const diffReviewStore = new DiffReviewStore();

export function gitDiffReviewItemId(afterOid: string, beforeOid: string | null | undefined, beforePath: string | null | undefined, path: string) {
  return `${afterOid}:${beforeOid ?? ''}:${beforePath ?? ''}:${path}`;
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
