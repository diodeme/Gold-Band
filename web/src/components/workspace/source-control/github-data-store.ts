import {
  getGitHubCapability,
  getGitHubIssue,
  getGitHubPullRequest,
  listGitHubIssues,
  listGitHubPullRequests,
} from '@/api';
import { BoundedLruCache } from '@/lib/bounded-lru-cache';
import { useCallback, useSyncExternalStore } from 'react';
import type {
  GitHubCapabilityVm,
  GitHubIssueDetailVm,
  GitHubIssueQueryVm,
  GitHubIssueSummaryVm,
  GitHubListStateVm,
  GitHubPullRequestDetailVm,
  GitHubPullRequestQueryVm,
  GitHubPullRequestSummaryVm,
} from '@/types';

interface GitHubDataApi {
  getCapability: typeof getGitHubCapability;
  listPullRequests: typeof listGitHubPullRequests;
  getPullRequest: typeof getGitHubPullRequest;
  listIssues: typeof listGitHubIssues;
  getIssue: typeof getGitHubIssue;
}

interface CacheSlot<T> {
  value?: T;
  request?: Promise<T>;
}

interface GitHubRepositoryCache {
  navigation: GitHubRepositoryNavigationVm;
  navigationListeners: Set<() => void>;
  capability: CacheSlot<GitHubCapabilityVm>;
  pullRequestLists: BoundedLruCache<string, CacheSlot<GitHubPullRequestSummaryVm[]>>;
  pullRequests: BoundedLruCache<string, CacheSlot<GitHubPullRequestDetailVm>>;
  issueLists: BoundedLruCache<string, CacheSlot<GitHubIssueSummaryVm[]>>;
  issues: BoundedLruCache<string, CacheSlot<GitHubIssueDetailVm>>;
}

export type GitHubRepositorySection = 'prs' | 'issues';
export type GitHubRepositoryDetailSection = 'overview' | 'files';
export type GitHubRepositorySelectionLocator = { kind: 'pr' | 'issue'; number: number };

export interface GitHubRepositoryNavigationVm {
  section: GitHubRepositorySection;
  listState: GitHubListStateVm;
  search: string;
  selection: GitHubRepositorySelectionLocator | null;
  detailSection: GitHubRepositoryDetailSection;
}

const DEFAULT_NAVIGATION: GitHubRepositoryNavigationVm = {
  section: 'prs',
  listState: 'open',
  search: '',
  selection: null,
  detailSection: 'overview',
};

const DEFAULT_API: GitHubDataApi = {
  getCapability: getGitHubCapability,
  listPullRequests: listGitHubPullRequests,
  getPullRequest: getGitHubPullRequest,
  listIssues: listGitHubIssues,
  getIssue: getGitHubIssue,
};

export class GitHubDataStore {
  static readonly MAX_REPOSITORIES = 24;
  static readonly MAX_LIST_QUERIES = 16;
  static readonly MAX_DETAILS = 48;

  private readonly repositories = new BoundedLruCache<string, GitHubRepositoryCache>(GitHubDataStore.MAX_REPOSITORIES);

  constructor(private readonly api: GitHubDataApi = DEFAULT_API) {}

  peekCapability(sessionKey: string) {
    return this.repositories.peek(sessionKey)?.capability.value ?? null;
  }

  navigation(sessionKey: string) {
    return this.repositories.peek(sessionKey)?.navigation ?? DEFAULT_NAVIGATION;
  }

  subscribeNavigation(sessionKey: string, listener: () => void) {
    const repository = this.repository(sessionKey);
    repository.navigationListeners.add(listener);
    return () => repository.navigationListeners.delete(listener);
  }

  setListContext(
    sessionKey: string,
    context: Partial<Pick<GitHubRepositoryNavigationVm, 'section' | 'listState' | 'search'>>,
  ) {
    this.updateNavigation(sessionKey, {
      ...context,
      selection: null,
      detailSection: 'overview',
    });
  }

  select(sessionKey: string, selection: GitHubRepositorySelectionLocator | null) {
    this.updateNavigation(sessionKey, { selection, detailSection: 'overview' });
  }

  setDetailSection(sessionKey: string, detailSection: GitHubRepositoryDetailSection) {
    this.updateNavigation(sessionKey, { detailSection });
  }

  peekPullRequest(sessionKey: string, host: string, repositoryName: string, number: number) {
    return this.repositories.peek(sessionKey)?.pullRequests.peek(detailKey(host, repositoryName, number))?.value ?? null;
  }

  peekIssue(sessionKey: string, host: string, repositoryName: string, number: number) {
    return this.repositories.peek(sessionKey)?.issues.peek(detailKey(host, repositoryName, number))?.value ?? null;
  }

  getCapability(
    sessionKey: string,
    projectId: string,
    workspacePath?: string | null,
    force = false,
  ) {
    const repository = this.repository(sessionKey);
    return loadCachedSlot(repository.capability, () => this.api.getCapability(projectId, workspacePath), force);
  }

  invalidateCapability(sessionKey: string) {
    const repository = this.repositories.peek(sessionKey);
    if (repository) repository.capability = {};
  }

  invalidateRepository(sessionKey: string) {
    const repository = this.repositories.peek(sessionKey);
    if (!repository) return;
    repository.capability = {};
    repository.pullRequestLists.clear();
    repository.pullRequests.clear();
    repository.issueLists.clear();
    repository.issues.clear();
  }

  listPullRequests(
    sessionKey: string,
    projectId: string,
    workspacePath: string | null | undefined,
    host: string,
    repositoryName: string,
    query: GitHubPullRequestQueryVm,
    force = false,
  ) {
    const cache = this.repository(sessionKey).pullRequestLists;
    const key = pullRequestQueryKey(host, repositoryName, query);
    return loadCached(cache, key, () => this.api.listPullRequests(
      projectId,
      workspacePath,
      host,
      repositoryName,
      query,
    ), force);
  }

  getPullRequest(
    sessionKey: string,
    projectId: string,
    workspacePath: string | null | undefined,
    host: string,
    repositoryName: string,
    number: number,
    force = false,
  ) {
    return loadCached(
      this.repository(sessionKey).pullRequests,
      detailKey(host, repositoryName, number),
      () => this.api.getPullRequest(projectId, workspacePath, host, repositoryName, number),
      force,
    );
  }

  listIssues(
    sessionKey: string,
    projectId: string,
    workspacePath: string | null | undefined,
    host: string,
    repositoryName: string,
    query: GitHubIssueQueryVm,
    force = false,
  ) {
    const cache = this.repository(sessionKey).issueLists;
    const key = issueQueryKey(host, repositoryName, query);
    return loadCached(cache, key, () => this.api.listIssues(
      projectId,
      workspacePath,
      host,
      repositoryName,
      query,
    ), force);
  }

  getIssue(
    sessionKey: string,
    projectId: string,
    workspacePath: string | null | undefined,
    host: string,
    repositoryName: string,
    number: number,
    force = false,
  ) {
    return loadCached(
      this.repository(sessionKey).issues,
      detailKey(host, repositoryName, number),
      () => this.api.getIssue(projectId, workspacePath, host, repositoryName, number),
      force,
    );
  }

  clear() {
    this.repositories.clear();
  }

  private repository(sessionKey: string) {
    let repository = this.repositories.get(sessionKey);
    if (!repository) {
      repository = {
        navigation: DEFAULT_NAVIGATION,
        navigationListeners: new Set(),
        capability: {},
        pullRequestLists: new BoundedLruCache(GitHubDataStore.MAX_LIST_QUERIES),
        pullRequests: new BoundedLruCache(GitHubDataStore.MAX_DETAILS),
        issueLists: new BoundedLruCache(GitHubDataStore.MAX_LIST_QUERIES),
        issues: new BoundedLruCache(GitHubDataStore.MAX_DETAILS),
      };
      this.repositories.set(sessionKey, repository);
    }
    return repository;
  }

  private updateNavigation(sessionKey: string, patch: Partial<GitHubRepositoryNavigationVm>) {
    const repository = this.repository(sessionKey);
    const navigation = { ...repository.navigation, ...patch };
    if (
      navigation.section === repository.navigation.section
      && navigation.listState === repository.navigation.listState
      && navigation.search === repository.navigation.search
      && navigation.selection?.kind === repository.navigation.selection?.kind
      && navigation.selection?.number === repository.navigation.selection?.number
      && navigation.detailSection === repository.navigation.detailSection
    ) return;
    repository.navigation = navigation;
    for (const listener of repository.navigationListeners) listener();
  }
}

function loadCached<K, T>(
  cache: BoundedLruCache<K, CacheSlot<T>>,
  key: K,
  loader: () => Promise<T>,
  force: boolean,
) {
  const existing = cache.get(key);
  if (existing?.request) return existing.request;
  if (!force && existing && 'value' in existing) return Promise.resolve(existing.value as T);
  const slot = existing ?? {};
  cache.set(key, slot);
  return loadCachedSlot(slot, loader, force, () => {
    if (!('value' in slot)) cache.delete(key);
  });
}

function loadCachedSlot<T>(
  slot: CacheSlot<T>,
  loader: () => Promise<T>,
  force: boolean,
  onRejected?: () => void,
) {
  if (slot.request) return slot.request;
  if (!force && 'value' in slot) return Promise.resolve(slot.value as T);
  const request = loader().then((value) => {
    slot.value = value;
    slot.request = undefined;
    return value;
  }).catch((reason: unknown) => {
    slot.request = undefined;
    onRejected?.();
    throw reason;
  });
  slot.request = request;
  return request;
}

function pullRequestQueryKey(host: string, repository: string, query: GitHubPullRequestQueryVm) {
  return [host, repository, query.state, query.search ?? '', query.author ?? '', query.base ?? '', query.head ?? '', query.label ?? ''].join('\u0000');
}

function issueQueryKey(host: string, repository: string, query: GitHubIssueQueryVm) {
  return [host, repository, query.state, query.search ?? '', query.author ?? '', query.assignee ?? '', query.label ?? '', query.milestone ?? ''].join('\u0000');
}

function detailKey(host: string, repository: string, number: number) {
  return [host, repository, number].join('\u0000');
}

export function githubRepositorySessionKey(
  projectId: string,
  repositoryCommonDir: string,
  workspacePath: string,
) {
  return [projectId, normalizePath(repositoryCommonDir), normalizePath(workspacePath)].join('\u0000');
}

function normalizePath(path: string) {
  const normalized = path.replaceAll('\\', '/').replace(/\/$/u, '');
  return /^[a-z]:\//iu.test(normalized) ? normalized.toLowerCase() : normalized;
}

export const githubDataStore = new GitHubDataStore();

export function useGitHubRepositoryNavigation(sessionKey: string) {
  const subscribe = useCallback(
    (listener: () => void) => githubDataStore.subscribeNavigation(sessionKey, listener),
    [sessionKey],
  );
  const getSnapshot = useCallback(() => githubDataStore.navigation(sessionKey), [sessionKey]);
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
