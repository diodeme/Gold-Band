import { getGitComparison } from '@/api';
import { BoundedLruCache } from '@/lib/bounded-lru-cache';
import type { GitComparisonSourceVm, GitFileComparisonVm } from '@/types';

type GitHubPullRequestComparisonSource = Extract<GitComparisonSourceVm, { kind: 'github-pr' }>;

interface CacheSlot {
  value?: GitFileComparisonVm;
  request?: Promise<GitFileComparisonVm>;
}

export class GitHubComparisonCache {
  static readonly MAX_COMPARISONS = 96;

  private readonly entries = new BoundedLruCache<string, CacheSlot>(GitHubComparisonCache.MAX_COMPARISONS);
  private readonly getComparison: typeof getGitComparison;

  constructor(getComparison?: typeof getGitComparison) {
    this.getComparison = getComparison ?? ((projectId, source) => getGitComparison(projectId, source));
  }

  get(projectId: string, source: GitHubPullRequestComparisonSource) {
    const key = comparisonKey(projectId, source);
    const existing = this.entries.get(key);
    if (existing?.request) return existing.request;
    if (existing?.value) return Promise.resolve(existing.value);
    const slot = existing ?? {};
    const request = this.getComparison(projectId, source).then((value) => {
      slot.value = value;
      slot.request = undefined;
      return value;
    }).catch((reason: unknown) => {
      if (this.entries.peek(key) === slot) this.entries.delete(key);
      throw reason;
    });
    slot.request = request;
    this.entries.set(key, slot);
    return request;
  }

  clear() {
    this.entries.clear();
  }
}

function comparisonKey(projectId: string, source: GitHubPullRequestComparisonSource) {
  return [projectId, source.workspacePath ?? '', source.host, source.repository, source.prNumber, source.baseOid, source.headOid, source.beforePath ?? '', source.path].join('\u0000');
}

export const githubComparisonCache = new GitHubComparisonCache();
