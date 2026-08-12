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
  SourceControlRepositoryView: ({ activeTab, onTabChange }: { activeTab: string; onTabChange: (tab: string) => void }) => <div data-tested-repository-view={activeTab}><button type="button" onClick={() => onTabChange('stashes')}>select stashes</button></div>,
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
      setRepositoryTab: vi.fn((_projectId: string, _workspacePath: string | null, repositoryTab: string) => update({ repositoryTab })),
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
      dismissOperationResult: vi.fn(),
      initializeRepository: vi.fn(),
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
  githubRuntime.getCapability.mockResolvedValue({ status: 'not-installed' });
});

afterEach(() => {
  document.body.replaceChildren();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe('source control history cache presentation', () => {
  it('shows distinct Git installation and repository initialization states', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      sessionRuntime.session = {
        ...sourceControlSession(), status: 'unavailable', snapshot: null, history: null,
        capability: { status: 'repository-required', repoRoot: null, commonDir: null, head: null },
      };
      await act(async () => root.render(<RightWorkspaceProvider><SourceControlWorkspacePanel resource={{
        kind: 'source-control', key: 'source-control:project-1:main', scopeKey: 'draft:default', title: 'Source control', attention: false, projectId: 'project-1', workspacePath: 'D:/repo',
      }} /></RightWorkspaceProvider>));
      expect(container.textContent).toContain('sourceControl.repositoryRequired');
      const initialize = Array.from(container.querySelectorAll<HTMLButtonElement>('button')).find((button) => button.textContent === 'sourceControl.initializeRepository');
      await act(async () => initialize?.click());
      expect(sourceControlStore.initializeRepository).toHaveBeenCalledWith('project-1', 'D:/repo');

      sessionRuntime.session = {
        ...sourceControlSession(), status: 'unavailable', snapshot: null, history: null,
        capability: { status: 'not-installed', repoRoot: null, commonDir: null, head: null },
      };
      await act(async () => { for (const listener of sessionRuntime.listeners) listener(); });
      expect(container.textContent).toContain('sourceControl.gitNotInstalled');
      expect(container.textContent).toContain('sourceControl.openGitDownload');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('places synchronization and stash-create actions in Changes and keeps Repository focused on resources', async () => {
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

      expect(container.querySelector('[data-source-control-changes-toolbar="true"]')).not.toBeNull();
      expect(container.querySelector('button[aria-label="sourceControl.fetch"]')).not.toBeNull();
      expect(container.querySelector('button[aria-label="sourceControl.push"]')).not.toBeNull();

      const repositoryTab = Array.from(container.querySelectorAll<HTMLButtonElement>('[role="tab"]'))
        .find((tab) => tab.textContent === 'sourceControl.repository');
      await act(async () => repositoryTab?.click());
      expect(container.querySelector('[data-source-control-changes-toolbar="true"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('disables a no-op sync action while keeping Fetch available', async () => {
    sessionRuntime.session = {
      ...sessionRuntime.session,
      snapshot: {
        ...(sessionRuntime.session?.snapshot as Record<string, unknown>),
        repository: {
          ...((sessionRuntime.session?.snapshot as { repository: Record<string, unknown> }).repository),
          upstream: { name: 'origin/main', ahead: 0, behind: 0 },
        },
      },
    };
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
      expect(container.querySelector<HTMLButtonElement>('button[aria-label="sourceControl.push"]')?.disabled).toBe(true);
      expect(container.querySelector<HTMLButtonElement>('button[aria-label="sourceControl.pull"]')).toBeNull();
      expect(container.querySelector<HTMLButtonElement>('button[aria-label="sourceControl.fetch"]')?.disabled).toBe(false);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('prioritizes Pull and shows both directions when the upstream is behind', async () => {
    sessionRuntime.session = {
      ...sessionRuntime.session,
      snapshot: {
        ...(sessionRuntime.session?.snapshot as Record<string, unknown>),
        repository: {
          ...((sessionRuntime.session?.snapshot as { repository: Record<string, unknown> }).repository),
          upstream: { name: 'origin/main', ahead: 2, behind: 3 },
        },
      },
    };
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
      const sync = container.querySelector<HTMLButtonElement>('button[aria-label="sourceControl.pull"]');
      expect(sync?.disabled).toBe(false);
      expect(sync?.textContent).toContain('↓3');
      expect(sync?.textContent).toContain('↑2');
      expect(container.querySelector('button[aria-label="sourceControl.push"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('renders terminal Git operation feedback until the user dismisses it', async () => {
    sessionRuntime.session = {
      ...sessionRuntime.session,
      activeOperation: {
        operationId: 'push-1', kind: 'push', repositoryCommonDir: 'D:/repo/.git', workspacePath: 'D:/repo',
        status: 'succeeded', cancelable: false, startedAt: '2026-08-12T00:00:00Z', completedAt: '2026-08-12T00:00:01Z', error: null,
      },
    };
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

      expect(container.querySelector('[data-source-control-operation-status="succeeded"]')).not.toBeNull();
      const dismiss = Array.from(container.querySelectorAll<HTMLButtonElement>('button'))
        .find((button) => button.getAttribute('aria-label') === 'sourceControl.dismissOperationResult');
      await act(async () => dismiss?.click());
      expect(sourceControlStore.dismissOperationResult).toHaveBeenCalledWith('project-1', 'D:/repo');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('shows the canonical Merge workflow and confirms continue through a typed operation', async () => {
    sessionRuntime.session = {
      ...sessionRuntime.session,
      snapshot: {
        ...(sessionRuntime.session?.snapshot as Record<string, unknown>),
        status: {
          ...((sessionRuntime.session?.snapshot as { status: Record<string, unknown> }).status),
          operationInProgress: { kind: 'merge', currentOid: null, currentSubject: null },
        },
      },
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<RightWorkspaceProvider><SourceControlWorkspacePanel resource={{
        kind: 'source-control', key: 'source-control:project-1:main', scopeKey: 'draft:default', title: 'Source control', attention: false, projectId: 'project-1', workspacePath: 'D:/repo',
      }} /></RightWorkspaceProvider>));
      const complete = Array.from(container.querySelectorAll<HTMLButtonElement>('button')).find((button) => button.textContent === 'sourceControl.conflictWorkflow.completeMerge');
      await act(async () => complete?.click());
      const confirm = Array.from(document.body.querySelectorAll<HTMLButtonElement>('button')).find((button) => button.textContent === 'common.confirm');
      await act(async () => confirm?.click());
      expect(sourceControlStore.startOperation).toHaveBeenCalledWith('project-1', 'D:/repo', { kind: 'merge-continue' });
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps the selected repository sub-tab in the source-control session', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<RightWorkspaceProvider><SourceControlWorkspacePanel resource={{
        kind: 'source-control', key: 'source-control:project-1:main', scopeKey: 'draft:default', title: 'Source control', attention: false, projectId: 'project-1', workspacePath: 'D:/repo',
      }} /></RightWorkspaceProvider>));
      const repository = Array.from(container.querySelectorAll<HTMLButtonElement>('[role="tab"]')).find((tab) => tab.textContent === 'sourceControl.repository');
      await act(async () => repository?.click());
      const selectStashes = Array.from(container.querySelectorAll<HTMLButtonElement>('button')).find((button) => button.textContent === 'select stashes');
      await act(async () => selectStashes?.click());
      expect(sessionRuntime.session?.repositoryTab).toBe('stashes');
      expect(sourceControlStore.setRepositoryTab).toHaveBeenCalledWith('project-1', 'D:/repo', 'stashes');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps the commit draft and file actions interactive during a background refresh', async () => {
    sessionRuntime.session = {
      ...sessionRuntime.session,
      refreshing: 'background',
      subject: 'draft subject',
      body: 'draft body',
    };
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

      const fetch = container.querySelector<HTMLButtonElement>('button[aria-label="sourceControl.fetch"]');
      const subject = container.querySelector<HTMLInputElement>('input[placeholder="sourceControl.commitSubject"]');
      const body = container.querySelector<HTMLTextAreaElement>('textarea[placeholder="sourceControl.commitBody"]');
      const stage = container.querySelector<HTMLButtonElement>('button[aria-label="sourceControl.stage: src/app.ts"]');
      expect(fetch?.disabled).toBe(false);
      expect(subject?.disabled).toBe(false);
      expect(body?.disabled).toBe(false);
      expect(stage?.disabled).toBe(false);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('shows progress only on the target file while a Stage mutation is running', async () => {
    sessionRuntime.session = {
      ...sessionRuntime.session,
      pendingAction: { kind: 'stage-paths', path: 'src/app.ts' },
    };
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

      const target = container.querySelector<HTMLButtonElement>('button[aria-label="sourceControl.stage: src/app.ts"]');
      const other = container.querySelector<HTMLButtonElement>('button[aria-label="sourceControl.stage: src/other.ts"]');
      const subject = container.querySelector<HTMLInputElement>('input[placeholder="sourceControl.commitSubject"]');
      expect(target?.getAttribute('aria-busy')).toBe('true');
      expect(target?.disabled).toBe(true);
      expect(other).toBeNull();
      expect(subject?.disabled).toBe(true);
    } finally {
      await act(async () => root.unmount());
    }
  });

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
    capability: { status: 'ready', repoRoot: 'D:/repo', commonDir: 'D:/repo/.git', head: '1'.repeat(40) },
    status: 'ready',
    activeOperation: null,
    activeTab: 'changes',
    repositoryTab: 'branches',
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
    refreshing: null,
    selectedCommitOids: new Set<string>(),
    selectionAnchorOid: null,
    snapshot: {
      repository: {
        projectId: 'project-1',
        commonDir: 'D:/repo/.git',
        workspacePath: 'D:/repo',
        currentBranch: 'main',
        upstream: { name: 'origin/main', ahead: 1, behind: 0 },
        remotes: [{ name: 'origin', fetchUrls: ['https://github.com/example/repo.git'], pushUrls: ['https://github.com/example/repo.git'] }],
        lock: { locked: false, operation: null },
      },
      status: { conflicts: [], staged: [], operationInProgress: null, unstaged: [
        { path: 'src/app.ts', oldPath: null, kind: 'modified', indexStatus: null, worktreeStatus: 'M', binary: false, submodule: false, addedLines: 1, deletedLines: 0 },
        { path: 'src/other.ts', oldPath: null, kind: 'modified', indexStatus: null, worktreeStatus: 'M', binary: false, submodule: false, addedLines: 2, deletedLines: 1 },
      ], untracked: [] },
      refs: [],
    },
    subject: '',
  };
}
