/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  getGitHubCapability: vi.fn(),
  listGitHubPullRequests: vi.fn(),
  getGitHubPullRequest: vi.fn(),
  listGitHubIssues: vi.fn(),
  getGitHubIssue: vi.fn(),
}));

vi.mock('@/api', () => ({
  ...api,
  cancelGitHubOperation: vi.fn(),
  openExternalUrl: vi.fn(),
  preflightGitHubPullRequest: vi.fn(),
  subscribeGitHubOperationUpdates: vi.fn().mockResolvedValue(() => undefined),
  startGitHubLogin: vi.fn(),
  startGitHubPullRequestCreate: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue ?? key }),
}));

vi.mock('@/components/workspace/files/WorkspaceFileEditor', () => ({
  WorkspaceFileEditor: ({ value }: { value: string }) => <div data-workspace-file-editor>{value}</div>,
}));

import { RightWorkspaceProvider } from '@/components/workspace/right-workspace-context';
import { SourceControlGitHubView } from '@/components/workspace/source-control/SourceControlGitHubView';
import { githubDataStore, githubRepositorySessionKey } from '@/components/workspace/source-control/github-data-store';
import type { GitHubPullRequestDetailVm, GitSourceControlSnapshotVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

beforeEach(() => {
  githubDataStore.clear();
  vi.clearAllMocks();
});

afterEach(() => {
  document.body.replaceChildren();
});

describe('source control GitHub navigation', () => {
  it('enters a visible detail loading state immediately after selecting a PR', async () => {
    const detailRequest = deferred<GitHubPullRequestDetailVm>();
    const detail = pullRequestDetail();
    api.getGitHubCapability.mockResolvedValue({
      status: 'ready',
      version: 'gh version 2.93.0',
      host: 'github.com',
      account: 'octocat',
      repository: 'acme/widgets',
      remote: 'origin',
      defaultBranch: 'main',
    });
    api.listGitHubPullRequests.mockResolvedValue([detail]);
    api.getGitHubPullRequest.mockReturnValue(detailRequest.promise);
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider>
            <SourceControlGitHubView
              projectId="project-1"
              workspacePath="D:/repo"
              snapshot={sourceControlSnapshot()}
              busy={false}
              onPush={() => undefined}
            />
          </RightWorkspaceProvider>,
        );
      });
      const row = Array.from(container.querySelectorAll('button'))
        .find((button) => button.textContent?.includes(detail.title));
      expect(row).not.toBeNull();

      await act(async () => row?.click());
      expect(container.querySelector('[data-source-control-github-detail-state="loading"]')).not.toBeNull();
      expect(api.getGitHubPullRequest).toHaveBeenCalledTimes(1);

      await act(async () => detailRequest.resolve(detail));
      expect(container.querySelector('[data-source-control-github-detail-state]')).toBeNull();
      expect(container.textContent).toContain(`#${detail.number} ${detail.title}`);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('bounds long PR metadata and file paths to the GitHub detail width', async () => {
    const detail = pullRequestDetail();
    detail.title = 'A very long pull request title '.repeat(12);
    detail.author = { login: 'a-very-long-github-account-name-that-must-not-grow-the-panel' };
    detail.headRefName = 'feature/a-very-long-branch-name-that-must-be-truncated';
    detail.baseRefName = 'release/a-very-long-target-branch-name';
    detail.files = [{ path: `docs/${'nested/'.repeat(20)}a-very-long-file-name.md`, oldPath: null, kind: 'modified', additions: 12, deletions: 3 }];
    api.getGitHubCapability.mockResolvedValue({
      status: 'ready', version: 'gh version 2.93.0', host: 'github.com', account: 'octocat', repository: 'acme/widgets', remote: 'origin', defaultBranch: 'main',
    });
    api.listGitHubPullRequests.mockResolvedValue([detail]);
    api.getGitHubPullRequest.mockResolvedValue(detail);
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => root.render(<RightWorkspaceProvider><SourceControlGitHubView projectId="project-1" workspacePath="D:/repo" snapshot={sourceControlSnapshot()} busy={false} onPush={() => undefined} /></RightWorkspaceProvider>));
      const row = Array.from(container.querySelectorAll('button')).find((button) => button.textContent?.includes(detail.title));
      await act(async () => row?.click());

      const detailRoot = container.querySelector('[data-source-control-github-detail="true"]');
      const title = container.querySelector('[data-source-control-github-detail-title="true"]');
      const meta = container.querySelector('[data-source-control-github-detail-meta="true"]');
      const branches = container.querySelector('[data-source-control-github-detail-branches="true"]');
      expect(detailRoot?.classList.contains('min-w-0')).toBe(true);
      expect(detailRoot?.classList.contains('overflow-hidden')).toBe(true);
      expect(title?.classList.contains('truncate')).toBe(true);
      expect(meta?.classList.contains('overflow-hidden')).toBe(true);
      expect(branches?.classList.contains('min-w-0')).toBe(true);
      expect(branches?.classList.contains('truncate')).toBe(true);

      await act(async () => githubDataStore.setDetailSection(githubRepositorySessionKey('project-1', 'D:/repo/.git', 'D:/repo'), 'files'));
      const fileRow = container.querySelector('[data-source-control-diff-file-row="true"]');
      expect(fileRow?.classList.contains('max-w-full')).toBe(true);
      expect(fileRow?.classList.contains('overflow-hidden')).toBe(true);
      expect(fileRow?.querySelector('button span.min-w-0 span')?.classList.contains('truncate')).toBe(true);
    } finally {
      await act(async () => root.unmount());
    }
  });
});

function sourceControlSnapshot() {
  return {
    repository: {
      commonDir: 'D:/repo/.git',
      workspacePath: 'D:/repo',
      currentBranch: 'main',
    },
    refs: [],
  } as GitSourceControlSnapshotVm;
}

function pullRequestDetail(): GitHubPullRequestDetailVm {
  return {
    number: 42,
    title: 'Show PR detail loading',
    state: 'OPEN',
    draft: false,
    author: { login: 'octocat' },
    headRefName: 'feature/loading',
    baseRefName: 'main',
    baseRefOid: '1'.repeat(40),
    headRefOid: '2'.repeat(40),
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
    files: [{ path: 'src/app.ts', oldPath: null, kind: 'modified', additions: 1, deletions: 1 }],
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
