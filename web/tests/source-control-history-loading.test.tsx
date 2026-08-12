/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const sessionRuntime = vi.hoisted(() => ({
  listeners: new Set<() => void>(),
  session: null as Record<string, unknown> | null,
}));
const githubRuntime = vi.hoisted(() => ({ getCapability: vi.fn() }));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/components/ui/tabs', async () => {
  const React = await import('react');
  const TabsContext = React.createContext<{ value: string; onValueChange: (value: string) => void } | null>(null);
  return {
    Tabs: ({ value, onValueChange, children }: { value: string; onValueChange: (value: string) => void; children: React.ReactNode }) => (
      <TabsContext.Provider value={{ value, onValueChange }}>{children}</TabsContext.Provider>
    ),
    TabsList: ({ children }: { children: React.ReactNode }) => <div role="tablist">{children}</div>,
    TabsTrigger: ({ value, children }: { value: string; children: React.ReactNode }) => {
      const context = React.useContext(TabsContext)!;
      return <button type="button" role="tab" aria-selected={context.value === value} onClick={() => context.onValueChange(value)}>{children}</button>;
    },
    TabsContent: ({ value, children }: { value: string; children: React.ReactNode }) => {
      const context = React.useContext(TabsContext)!;
      return context.value === value ? <div role="tabpanel">{children}</div> : null;
    },
  };
});

vi.mock('@/components/workspace/source-control/SourceControlHistoryView', () => ({
  SourceControlHistoryView: () => <div data-tested-history-view />,
}));

vi.mock('@/components/workspace/source-control/SourceControlRepositoryView', () => ({
  SourceControlRepositoryView: () => <div data-tested-repository-view />,
}));

vi.mock('@/components/workspace/source-control/SourceControlGitHubView', () => ({
  SourceControlGitHubView: () => <div data-tested-github-view />,
}));

vi.mock('@/components/workspace/source-control/github-data-store', () => ({
  githubDataStore: { getCapability: githubRuntime.getCapability },
  githubRepositorySessionKey: (projectId: string, commonDir: string, workspacePath: string) => `${projectId}:${commonDir}:${workspacePath}`,
}));

vi.mock('@/components/workspace/source-control/source-control-store', async () => {
  const React = await import('react');
  const update = (patch: Record<string, unknown>) => {
    sessionRuntime.session = { ...sessionRuntime.session, ...patch };
    for (const listener of sessionRuntime.listeners) listener();
  };
  return {
    sourceControlStore: {
      ensureLoaded: vi.fn(),
      refresh: vi.fn(),
      setActiveTab: vi.fn((_projectId: string, _workspacePath: string | null, activeTab: string) => update({ activeTab })),
      setHistoryPage: vi.fn(),
      selectCommit: vi.fn(),
      selectCommitForContextMenu: vi.fn(),
      clearCommitSelection: vi.fn(),
      setSubject: vi.fn(),
      setBody: vi.fn(),
      mutate: vi.fn(),
      loadMoreHistory: vi.fn(),
      loadCommitReachability: vi.fn(),
      closeCommitReachability: vi.fn(),
      startOperation: vi.fn(),
      cancelOperation: vi.fn(),
    },
    useSourceControlSession: () => React.useSyncExternalStore(
      (listener) => {
        sessionRuntime.listeners.add(listener);
        return () => sessionRuntime.listeners.delete(listener);
      },
      () => sessionRuntime.session,
      () => sessionRuntime.session,
    ),
  };
});

import { RightWorkspaceProvider } from '@/components/workspace/right-workspace-context';
import { SourceControlWorkspacePanel } from '@/components/workspace/source-control/SourceControlWorkspacePanel';
import { sourceControlStore } from '@/components/workspace/source-control/source-control-store';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

beforeEach(() => {
  sessionRuntime.listeners.clear();
  sessionRuntime.session = sourceControlSession();
});

afterEach(() => {
  document.body.replaceChildren();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe('source control history cache presentation', () => {
  it('prewarms GitHub capability once the source-control repository is ready', async () => {
    githubRuntime.getCapability.mockResolvedValue({ status: 'not-installed' });
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<RightWorkspaceProvider><SourceControlWorkspacePanel resource={{
          kind: 'source-control', key: 'source-control:project-1:main', scopeKey: 'draft:default',
          title: 'Source control', attention: false, projectId: 'project-1', workspacePath: 'D:/repo',
        }} /></RightWorkspaceProvider>);
      });
      expect(githubRuntime.getCapability).toHaveBeenCalledTimes(1);
      expect(githubRuntime.getCapability).toHaveBeenCalledWith(
        'project-1:D:/repo/.git:D:/repo', 'project-1', 'D:/repo',
      );
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('restores cached history immediately across source-control tab round trips', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider>
            <SourceControlWorkspacePanel resource={{
              kind: 'source-control',
              key: 'source-control:project-1:main',
              scopeKey: 'draft:default',
              title: 'Source control',
              attention: false,
              projectId: 'project-1',
              workspacePath: 'D:/repo',
            }} />
          </RightWorkspaceProvider>,
        );
      });

      const historyTab = Array.from(container.querySelectorAll<HTMLButtonElement>('[role="tab"]'))
        .find((tab) => tab.textContent === 'sourceControl.history');
      expect(historyTab).not.toBeNull();
      await act(async () => historyTab?.click());
      expect(sessionRuntime.session?.activeTab).toBe('history');
      expect(container.querySelector('[data-source-control-history-state="loading"]')).toBeNull();
      expect(container.querySelector('[data-tested-history-view]')).not.toBeNull();

      const repositoryTab = Array.from(container.querySelectorAll<HTMLButtonElement>('[role="tab"]'))
        .find((tab) => tab.textContent === 'sourceControl.repository');
      await act(async () => repositoryTab?.click());
      await act(async () => historyTab?.click());

      expect(container.querySelector('[data-source-control-history-state="loading"]')).toBeNull();
      expect(container.querySelector('[data-tested-history-view]')).not.toBeNull();
      expect(sourceControlStore.ensureLoaded).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => root.unmount());
    }
  });
});

function sourceControlSession() {
  const commit = {
    oid: '1'.repeat(40),
    parentOids: [],
    subject: 'feat: history',
    body: '',
    author: { name: 'Ada', email: null, timestamp: '2026-08-12T00:00:00Z' },
    committer: { name: 'Ada', email: null, timestamp: '2026-08-12T00:00:00Z' },
    refs: [],
    sourceRef: 'refs/heads/main',
    runtimeCheckpoint: false,
  };
  return {
    activeOperation: null,
    activeTab: 'changes',
    body: '',
    commitReview: null,
    commitReachability: null,
    error: null,
    focusedCommitOid: null,
    history: { commits: [commit], nextCursor: null, revision: 'revision-1' },
    historyDetailLoading: false,
    reachabilityLoading: false,
    historyPage: 0,
    pendingAction: null,
    selectedCommitOids: new Set<string>(),
    selectionAnchorOid: null,
    snapshot: {
      repository: {
        projectId: 'project-1',
        commonDir: 'D:/repo/.git',
        workspacePath: 'D:/repo',
        currentBranch: 'main',
        upstream: null,
        lock: { locked: false, operation: null },
      },
      status: { conflicts: [], staged: [], unstaged: [], untracked: [] },
      refs: [],
    },
    subject: '',
  };
}
