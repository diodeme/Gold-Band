import { describe, expect, it, vi } from 'vitest';
import { GitHubDataStore, githubRepositorySessionKey } from '@/components/workspace/source-control/github-data-store';
import type {
  GitHubCapabilityVm,
  GitHubPullRequestDetailVm,
} from '@/types';

describe('GitHub data store', () => {
  it('caches capability by repository workspace and deduplicates an in-flight detection', async () => {
    const pending = deferred<GitHubCapabilityVm>();
    const api = fakeApi();
    api.getCapability.mockReturnValue(pending.promise);
    const store = new GitHubDataStore(api);
    const sessionKey = githubRepositorySessionKey('project-1', 'D:/repo/.git', 'D:/repo');

    const first = store.getCapability(sessionKey, 'project-1', 'D:/repo');
    const duplicate = store.getCapability(sessionKey, 'project-1', 'D:/repo');
    expect(api.getCapability).toHaveBeenCalledTimes(1);
    expect(duplicate).toBe(first);

    const capability = readyCapability();
    pending.resolve(capability);
    await expect(first).resolves.toEqual(capability);
    await expect(store.getCapability(sessionKey, 'project-1', 'D:/repo')).resolves.toEqual(capability);
    expect(api.getCapability).toHaveBeenCalledTimes(1);

    api.getCapability.mockResolvedValueOnce(capability);
    await store.getCapability(sessionKey, 'project-1', 'D:/repo', true);
    expect(api.getCapability).toHaveBeenCalledTimes(2);
  });

  it('reuses PR lists and details until an explicit refresh is requested', async () => {
    const api = fakeApi();
    const detail = pullRequestDetail();
    api.listPullRequests.mockResolvedValue([detail]);
    api.getPullRequest.mockResolvedValue(detail);
    const store = new GitHubDataStore(api);
    const sessionKey = githubRepositorySessionKey('project-1', 'D:/repo/.git', 'D:/repo');
    const query = { state: 'open' as const, search: null, author: null, base: null, head: null, label: null };

    await store.listPullRequests(sessionKey, 'project-1', 'D:/repo', 'github.com', 'acme/widgets', query);
    await store.listPullRequests(sessionKey, 'project-1', 'D:/repo', 'github.com', 'acme/widgets', query);
    await store.getPullRequest(sessionKey, 'project-1', 'D:/repo', 'github.com', 'acme/widgets', 42);
    await store.getPullRequest(sessionKey, 'project-1', 'D:/repo', 'github.com', 'acme/widgets', 42);
    expect(api.listPullRequests).toHaveBeenCalledTimes(1);
    expect(api.getPullRequest).toHaveBeenCalledTimes(1);

    await store.listPullRequests(sessionKey, 'project-1', 'D:/repo', 'github.com', 'acme/widgets', query, true);
    await store.getPullRequest(sessionKey, 'project-1', 'D:/repo', 'github.com', 'acme/widgets', 42, true);
    expect(api.listPullRequests).toHaveBeenCalledTimes(2);
    expect(api.getPullRequest).toHaveBeenCalledTimes(2);
  });

  it('keeps repository-scoped GitHub navigation locators across view unmounts and data invalidation', async () => {
    const api = fakeApi();
    const detail = pullRequestDetail();
    api.getPullRequest.mockResolvedValue(detail);
    const store = new GitHubDataStore(api);
    const sessionKey = githubRepositorySessionKey('project-1', 'D:/repo/.git', 'D:/repo');
    const listener = vi.fn();
    const unsubscribe = store.subscribeNavigation(sessionKey, listener);

    store.setListContext(sessionKey, { section: 'prs', listState: 'all', search: 'cache' });
    store.select(sessionKey, { kind: 'pr', number: 42 });
    store.setDetailSection(sessionKey, 'files');
    unsubscribe();

    await store.getPullRequest(sessionKey, 'project-1', 'D:/repo', 'github.com', 'acme/widgets', 42);
    expect(store.peekPullRequest(sessionKey, 'github.com', 'acme/widgets', 42)).toEqual(detail);
    expect(store.navigation(sessionKey)).toEqual({
      section: 'prs',
      listState: 'all',
      search: 'cache',
      selection: { kind: 'pr', number: 42 },
      detailSection: 'files',
    });
    expect(listener).toHaveBeenCalledTimes(3);

    store.invalidateRepository(sessionKey);
    expect(store.peekPullRequest(sessionKey, 'github.com', 'acme/widgets', 42)).toBeNull();
    expect(store.navigation(sessionKey)).toMatchObject({
      selection: { kind: 'pr', number: 42 },
      detailSection: 'files',
    });
  });

});

function fakeApi() {
  return {
    getCapability: vi.fn(),
    listPullRequests: vi.fn(),
    getPullRequest: vi.fn(),
    listIssues: vi.fn(),
    getIssue: vi.fn(),
  };
}

function readyCapability(): GitHubCapabilityVm {
  return {
    status: 'ready',
    version: 'gh version 2.93.0',
    host: 'github.com',
    account: 'octocat',
    repository: 'acme/widgets',
    remote: 'origin',
    defaultBranch: 'main',
  };
}

function pullRequestDetail(): GitHubPullRequestDetailVm {
  return {
    number: 42,
    title: 'Cache GitHub data',
    state: 'OPEN',
    draft: false,
    author: { login: 'octocat' },
    headRefName: 'feature/cache',
    baseRefName: 'main',
    baseRefOid: '1111111111111111111111111111111111111111',
    headRefOid: '2222222222222222222222222222222222222222',
    updatedAt: '2026-08-11T00:00:00Z',
    url: 'https://github.com/acme/widgets/pull/42',
    reviewDecision: null,
    labels: [],
    statusChecks: [],
    body: 'Body',
    mergeable: 'MERGEABLE',
    mergeStateStatus: 'CLEAN',
    additions: 1,
    deletions: 1,
    changedFiles: 1,
    files: [{ path: 'src/app.ts', additions: 1, deletions: 1 }],
    latestReviews: [],
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((nextResolve, nextReject) => {
    resolve = nextResolve;
    reject = nextReject;
  });
  return { promise, resolve, reject };
}
