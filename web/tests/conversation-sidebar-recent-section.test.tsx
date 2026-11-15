/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/components/ui/scroll-area', () => ({
  ScrollArea: ({ children, ...props }: { children: React.ReactNode } & Record<string, unknown>) => (
    <div {...props}>{children}</div>
  ),
}));

import { ConversationSidebar, conversationSidebarRunKey, doesConversationSidebarRunListOwnSelection } from '@/components/conversation/ConversationSidebar';
import { applyConversationRecentTaskPage, createEmptyConversationSidebar } from '@/lib/conversation-sidebar-loading';
import type { ConversationPage, ConversationSidebarVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function recentSidebar(nextCursor: string | null) {
  const sidebar = createEmptyConversationSidebar();
  return applyConversationRecentTaskPage(sidebar, {
    tasks: [
      {
        projectId: 'project-a', taskId: 'task-1', taskUuid: 'uuid-1', title: 'Recent A', autoTitle: false,
        runMode: 'direct', latestRun: { runId: 'run-9', status: 'completed', updatedAt: '2026-10-01T00:00:00Z' } as never,
        runs: [], runHistoryStatus: 'not-loaded', runsNextCursor: null, pinned: false, workspaceName: 'Workspace A',
      },
      {
        projectId: 'project-b', taskId: 'task-2', taskUuid: 'uuid-2', title: 'Recent B (no run)', autoTitle: false,
        runMode: 'workflow', latestRun: null, runs: [], runHistoryStatus: 'not-loaded', runsNextCursor: null,
        pinned: false, workspaceName: 'Workspace B',
      },
    ],
    nextCursor,
    errors: [],
  }, false);
}

function mount(vm: ConversationSidebarVm, onSelect: (page: ConversationPage) => void) {
  const container = document.createElement('div');
  document.body.append(container);
  const root: Root = createRoot(container);
  act(() => {
    root.render(
      <ConversationSidebar
        vm={vm}
        active={{ kind: 'conversation-home' }}
        onSelect={onSelect}
        onNewConversation={() => {}}
        onSearch={() => {}}
        onPinTask={() => {}}
        onUnpinTask={() => {}}
        onRenameTask={() => {}}
        onDeleteTask={() => {}}
        onRetryBootstrap={() => {}}
        onRequestWorkspaceTasks={() => {}}
        onRequestPinnedTasks={() => {}}
        onRequestRecentTasks={() => {}}
        onRequestTaskRuns={() => {}}
      />,
    );
  });
  return { container, root };
}

afterEach(() => {
  document.body.replaceChildren();
});

describe('ConversationSidebar recent section', () => {
  it('renders as the last group of the shared scroll tree without structural separators', () => {
    // 2026-11-05 IA: pinned / workspaces / recent are groups of one scrolling
    // sidebar tree — no fixed-height recent tail, no boundary separators; the
    // sticky group headings carry the visual structure.
    const vm = recentSidebar('cursor-2');
    vm.workspaces = [
      { projectId: 'project-a', workspacePath: '/a', name: 'Workspace A' },
      { projectId: 'project-b', workspacePath: '/b', name: 'Workspace B' },
    ];
    const { container, root } = mount(vm, () => {});
    try {
      const recentHeading = container.querySelector('[data-conversation-sidebar-heading="recent"]')!;
      const scrollRegion = container.querySelector('[data-conversation-sidebar-region="scrollable-conversations"]')!;
      const recentRegion = container.querySelector('[data-conversation-sidebar-region="recent"]')!;
      const workspacesRegion = container.querySelector('[data-conversation-sidebar-region="workspaces"]')!;
      expect(scrollRegion.contains(recentHeading)).toBe(true);
      expect(recentRegion.contains(recentHeading)).toBe(true);
      // Order: workspaces group first, recent group last.
      expect(
        workspacesRegion.compareDocumentPosition(recentRegion) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
      // No bounded internal scroller — recent rows flow with the whole tree.
      expect(recentRegion.querySelector('.gold-themed-scrollbar')).toBeNull();
      expect(recentRegion.querySelector('.h-\\[clamp')) .toBeNull();
      // Group boundaries inside the scroll tree are headings, never separators;
      // the only structural separator sits between the fixed navigation and
      // the tree (restored 2026-11-06 after the IA pass dropped it).
      expect(scrollRegion.querySelectorAll('[data-conversation-sidebar-separator]').length).toBe(0);
      const navigationSeparator = container.querySelector('[data-conversation-sidebar-separator="navigation"]')!;
      const fixedNavigation = container.querySelector('[data-conversation-sidebar-region="fixed-navigation"]')!;
      expect(navigationSeparator).toBeTruthy();
      expect(fixedNavigation.contains(navigationSeparator)).toBe(false);
      expect(scrollRegion.contains(navigationSeparator)).toBe(false);
      expect(
        fixedNavigation.compareDocumentPosition(scrollRegion) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    } finally {
      act(() => root.unmount());
    }

    // A pinned section also becomes a plain group: heading present, still no
    // structural separators inside the tree.
    const withPins = recentSidebar(null);
    withPins.pinRefs = [{ projectId: 'project-a', taskId: 'task-1', taskUuid: 'uuid-1' } as never];
    const pinned = mount(withPins, () => {});
    try {
      expect(pinned.container.querySelector('[data-conversation-sidebar-heading="pinned"]')).toBeTruthy();
      expect(pinned.container.querySelector('[data-conversation-sidebar-region="scrollable-conversations"]')!
        .querySelectorAll('[data-conversation-sidebar-separator]').length).toBe(0);
    } finally {
      act(() => pinned.root.unmount());
    }
  });

  it('renders with a single workspace (always visible)', () => {
    const vm = {
      ...recentSidebar(null),
      workspaces: [{ projectId: 'project-a', workspacePath: '/a', name: 'Workspace A' }],
    };
    const { container, root } = mount(vm, () => {});
    try {
      expect(container.querySelector('[data-conversation-sidebar-heading="recent"]')).toBeTruthy();
      expect(container.querySelector('[data-conversation-task-row="recent"][data-task-id="task-1"]')).toBeTruthy();
    } finally {
      act(() => root.unmount());
    }
  });

  it('shows an end hint only when a paginated list is exhausted', () => {
    // Exhausted with a full page of rows -> the end hint confirms "no more".
    const paginated = recentSidebar(null);
    paginated.recentTasks = Array.from({ length: 10 }, (_, index) => ({
      ...paginated.recentTasks[0],
      taskId: `task-${index}`,
      taskUuid: `uuid-${index}`,
    }));
    const first = mount(paginated, () => {});
    try {
      expect(first.container.textContent).toContain('conversation.sidebar.allRecentShown');
    } finally {
      act(() => first.root.unmount());
    }
    // Exhausted with fewer rows than one page -> silent end, no noise.
    const short = recentSidebar(null);
    const second = mount(short, () => {});
    try {
      expect(second.container.textContent).not.toContain('conversation.sidebar.allRecentShown');
      expect(second.container.textContent).not.toContain('conversation.sidebar.loadMoreRecent');
    } finally {
      act(() => second.root.unmount());
    }
  });

  it('shows the load-more button only while more pages remain (no auto-load)', () => {
    // 2026-11-06: scrolling never pages the sidebar lists — only the explicit
    // button loads more (user feedback: scroll-driven growth feels out of
    // control in a navigation tree).
    const more = recentSidebar('cursor-2');
    const exhausted = recentSidebar(null);
    const first = mount(more, () => {});
    try {
      expect(first.container.textContent).toContain('conversation.sidebar.loadMoreRecent');
      expect(first.container.querySelector('[data-load-more-sentinel]')).toBeNull();
    } finally {
      act(() => first.root.unmount());
    }
    const second = mount(exhausted, () => {});
    try {
      expect(second.container.textContent).not.toContain('conversation.sidebar.loadMoreRecent');
      expect(second.container.querySelector('[data-load-more-sentinel]')).toBeNull();
    } finally {
      act(() => second.root.unmount());
    }
  });

  it('does not claim an empty recent list while more pages remain', () => {
    // A page whose every slot failed server-side yields zero rows but still a
    // cursor: the section keeps loading instead of declaring "no recent".
    const emptyWithCursor = recentSidebar('cursor-2');
    emptyWithCursor.recentTasks = [];
    const first = mount(emptyWithCursor, () => {});
    try {
      expect(first.container.textContent).not.toContain('conversation.sidebar.noRecent');
      expect(first.container.textContent).toContain('conversation.sidebar.loadMoreRecent');
    } finally {
      act(() => first.root.unmount());
    }
    // Genuinely exhausted and empty -> the empty state shows.
    const emptyExhausted = recentSidebar(null);
    emptyExhausted.recentTasks = [];
    const second = mount(emptyExhausted, () => {});
    try {
      expect(second.container.textContent).toContain('conversation.sidebar.noRecent');
    } finally {
      act(() => second.root.unmount());
    }
  });

  it('navigates with the full locator and tolerates rows without a run', () => {
    const onSelect = vi.fn();
    const { container, root } = mount(recentSidebar(null), onSelect);
    try {
      const row = container.querySelector('[data-conversation-task-row="recent"][data-task-id="task-1"]') as HTMLElement;
      act(() => {
        row.click();
      });
      expect(onSelect).toHaveBeenCalledTimes(1);
      expect(onSelect).toHaveBeenCalledWith({
        kind: 'conversation-run',
        projectId: 'project-a',
        taskId: 'task-1',
        taskUuid: 'uuid-1',
        runId: 'run-9',
      });

      // A row whose task has no run yet must not navigate or throw, and must
      // not look interactive: no cursor/hover affordances, no hover actions
      // (parity with the former static recent row).
      const noRun = container.querySelector('[data-conversation-task-row="recent"][data-task-id="task-2"]') as HTMLElement;
      expect(noRun.classList.contains('cursor-pointer')).toBe(false);
      expect(noRun.className).not.toContain('hover:bg-sidebar-accent');
      act(() => {
        noRun.click();
      });
      expect(onSelect).toHaveBeenCalledTimes(1);
    } finally {
      act(() => root.unmount());
    }
  });

  it('expands and navigates run history on workflow rows like the workspace list', () => {
    // 2026-11-06: recent rows reuse the workspace task row, so a workflow
    // conversation opens its run list right in the recent group and selecting
    // a run navigates with the run locator.
    const onSelect = vi.fn();
    const vm = recentSidebar(null);
    vm.recentTasks[1] = {
      ...vm.recentTasks[1],
      latestRun: { runId: 'run-7', status: 'completed', updatedAt: '2026-10-02T00:00:00Z' } as never,
      runs: [
        { runId: 'run-7', status: 'completed', updatedAt: '2026-10-02T00:00:00Z' },
        { runId: 'run-8', status: 'completed', updatedAt: '2026-10-03T00:00:00Z' },
      ] as never,
      runHistoryStatus: 'ready',
      runsNextCursor: null,
    };
    const { container, root } = mount(vm, onSelect);
    try {
      const row = container.querySelector('[data-conversation-task-row="recent"][data-task-id="task-2"]') as HTMLElement;
      act(() => {
        row.click();
      });
      // Clicking a workflow row expands its runs and navigates the latest run.
      expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ taskId: 'task-2', runId: 'run-7' }));
      expect(container.textContent).toContain('run-8');

      const runRow = [...container.querySelectorAll<HTMLElement>('div.rounded-md')]
        .find((element) => element.textContent?.includes('run-8'));
      expect(runRow).toBeTruthy();
      act(() => {
        runRow!.click();
      });
      expect(onSelect).toHaveBeenLastCalledWith(expect.objectContaining({ taskId: 'task-2', runId: 'run-8' }));
    } finally {
      act(() => root.unmount());
    }
  });

  it('sinks the selection accent to the visible run row, keeping exactly one highlight', () => {
    // 2026-11-07: while the active run row is rendered inside the expanded
    // run list, it alone carries the accent; the task row keeps only its
    // weight. Collapsing the list lifts the accent back to the task row.
    const onSelect = vi.fn();
    const vm = recentSidebar(null);
    vm.recentTasks[1] = {
      ...vm.recentTasks[1],
      latestRun: { runId: 'run-7', status: 'completed', updatedAt: '2026-10-02T00:00:00Z' } as never,
      runs: [
        { runId: 'run-7', status: 'completed', updatedAt: '2026-10-02T00:00:00Z' },
        { runId: 'run-8', status: 'completed', updatedAt: '2026-10-03T00:00:00Z' },
      ] as never,
      runHistoryStatus: 'ready',
      runsNextCursor: null,
    };
    const { container, root } = mount(vm, onSelect);
    const activePage: ConversationPage = {
      kind: 'conversation-run', projectId: 'project-b', taskId: 'task-2', taskUuid: 'uuid-2', runId: 'run-7',
    };
    try {
      const row = container.querySelector('[data-conversation-task-row="recent"][data-task-id="task-2"]') as HTMLElement;
      act(() => {
        row.click();
      });
      expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ runId: 'run-7' }));
      act(() => {
        root.render(
          <ConversationSidebar
            vm={vm}
            active={activePage}
            onSelect={onSelect}
            onNewConversation={() => {}}
            onSearch={() => {}}
            onPinTask={() => {}}
            onUnpinTask={() => {}}
            onRenameTask={() => {}}
            onDeleteTask={() => {}}
            onRetryBootstrap={() => {}}
            onRequestWorkspaceTasks={() => {}}
            onRequestPinnedTasks={() => {}}
            onRequestRecentTasks={() => {}}
            onRequestTaskRuns={() => {}}
          />,
        );
      });
      const expandedRow = container.querySelector('[data-conversation-task-row="recent"][data-task-id="task-2"]') as HTMLElement;
      const activeRunRow = [...container.querySelectorAll<HTMLElement>('div.rounded-md')]
        .find((element) => element.textContent?.includes('run-7'));
      expect(activeRunRow).toBeTruthy();
      // The run row owns the accent; the task row keeps weight but no fill.
      expect(activeRunRow!.className).toContain('bg-sidebar-accent text-sidebar-accent-foreground');
      expect(expandedRow.className).toContain('font-medium');
      expect(expandedRow.className).not.toContain('bg-sidebar-accent/70');
      // Collapsing the run list returns the accent to the task row.
      act(() => {
        expandedRow.click();
      });
      const collapsedRow = container.querySelector('[data-conversation-task-row="recent"][data-task-id="task-2"]') as HTMLElement;
      expect(collapsedRow.className).toContain('bg-sidebar-accent/70');
    } finally {
      act(() => root.unmount());
    }
  });

  it('keeps the recent scope when run history loads after a recent click (no workspace steal)', () => {
    // 2026-11-07 regression: the interaction marker was consumed on the first
    // effect run; the VM refresh from the click-triggered run-history load
    // re-ran the effect with an empty marker and misclassified it as external
    // activation, stealing the highlight to the workspace copy.
    const onSelect = vi.fn();
    const requestRuns = vi.fn();
    const task = {
      projectId: 'project-b', taskId: 'task-2', taskUuid: 'uuid-2', title: 'Recent B', autoTitle: false,
      runMode: 'workflow', latestRun: { runId: 'run-7', status: 'completed', updatedAt: '2026-10-02T00:00:00Z' } as never,
      runs: [], runHistoryStatus: 'not-loaded' as const, runsNextCursor: null, pinned: false, workspaceName: 'Workspace B',
    };
    const buildVm = (loaded: boolean): ConversationSidebarVm => {
      const base = recentSidebar(null);
      const copy = (runs: typeof task.runs, status: typeof task.runHistoryStatus) => ({
        ...task, runs, runHistoryStatus: status,
      });
      const runs = (loaded
        ? [{ runId: 'run-7', status: 'completed', updatedAt: '2026-10-02T00:00:00Z' }, { runId: 'run-8', status: 'completed', updatedAt: '2026-10-03T00:00:00Z' }] as never
        : []);
      return {
        ...base,
        loadStatus: 'ready' as const,
        preferences: { 'workspaces.collapsed': false },
        workspaces: [{ projectId: 'project-b', name: 'Workspace B', workspacePath: 'D:/b' }],
        tasksByWorkspace: { 'project-b': [copy(runs, loaded ? 'ready' : 'not-loaded')] },
        workspaceTaskPages: { 'project-b': { status: 'ready' as const, nextCursor: null } },
        recentTasks: [copy(runs, loaded ? 'ready' : 'not-loaded')],
        recentTaskPage: { status: 'ready' as const, nextCursor: null },
      };
    };
    const container0 = document.createElement('div');
    document.body.append(container0);
    const root: Root = createRoot(container0);
    const props = (vm: ConversationSidebarVm, active: ConversationPage) => (
      <ConversationSidebar
        vm={vm}
        active={active}
        onSelect={onSelect}
        onNewConversation={() => {}}
        onSearch={() => {}}
        onPinTask={() => {}}
        onUnpinTask={() => {}}
        onRenameTask={() => {}}
        onDeleteTask={() => {}}
        onRetryBootstrap={() => {}}
        onRequestWorkspaceTasks={() => {}}
        onRequestPinnedTasks={() => {}}
        onRequestRecentTasks={() => {}}
        onRequestTaskRuns={requestRuns}
      />
    );
    const activePage: ConversationPage = {
      kind: 'conversation-run', projectId: 'project-b', taskId: 'task-2', taskUuid: 'uuid-2', runId: 'run-7',
    };
    try {
      const initialVm = buildVm(false);
      act(() => { root.render(props(initialVm, { kind: 'conversation-home' })); });
      const row = container0.querySelector('[data-conversation-task-row="recent"][data-task-id="task-2"]') as HTMLElement;
      act(() => { row.click(); });
      expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ runId: 'run-7' }));
      // Commit 1: App switches the active page (same vm instance — the run
      // history request is still in flight).
      act(() => { root.render(props(initialVm, activePage)); });
      // Commit 2: the run-history response lands; every task copy gets fresh
      // object identities, including tasksByWorkspace.
      act(() => { root.render(props(buildVm(true), activePage)); });
      const recentRegion = container0.querySelector('[data-conversation-sidebar-region="recent"]') as HTMLElement;
      const recentRunRow = [...recentRegion.querySelectorAll<HTMLElement>('div.rounded-md')]
        .find((element) => element.textContent?.includes('run-7')) as HTMLElement;
      expect(recentRunRow).toBeTruthy();
      // The active run class combo, not the hover class that also contains
      // the substring.
      expect(recentRunRow.className).toContain('bg-sidebar-accent text-sidebar-accent-foreground');
      const workspaceRegion = container0.querySelector('[data-conversation-workspace-group="project-b"]') as HTMLElement;
      // A recent click never expands the workspace copy's run list.
      const workspaceRunRow = [...workspaceRegion.querySelectorAll<HTMLElement>('div.rounded-md')]
        .find((element) => element.textContent?.includes('run-7'));
      expect(workspaceRunRow).toBeUndefined();
      const workspaceRow = container0.querySelector('[data-conversation-task-row="workspace"][data-task-id="task-2"]') as HTMLElement;
      expect(workspaceRow.className).not.toContain('font-medium');
    } finally {
      act(() => { root.unmount(); });
    }
  });

  it('keeps the task-row accent when the active run is not rendered in the list', () => {
    // 2026-11-07: an active run beyond the loaded page (or a null run key)
    // leaves the accent on the task row — the deepest VISIBLE node owns it.
    const task = { projectId: 'p', taskId: 't', taskUuid: null, runs: [{ runId: 'run-1' }] };
    const loadedKey = conversationSidebarRunKey('p', 't', 'run-1', null);
    const unloadedKey = conversationSidebarRunKey('p', 't', 'run-9', null);
    expect(doesConversationSidebarRunListOwnSelection(task, loadedKey, true)).toBe(true);
    expect(doesConversationSidebarRunListOwnSelection(task, unloadedKey, true)).toBe(false);
    expect(doesConversationSidebarRunListOwnSelection(task, loadedKey, false)).toBe(false);
    expect(doesConversationSidebarRunListOwnSelection(task, null, true)).toBe(false);
  });

  it('collapsing the recent group resets it to the default first page', () => {
    const onResetRecentTaskPage = vi.fn();
    const vm = recentSidebar('cursor-2');
    const container = document.createElement('div');
    document.body.append(container);
    const root: Root = createRoot(container);
    act(() => {
      root.render(
        <ConversationSidebar
          vm={vm}
          active={{ kind: 'conversation-home' }}
          onSelect={() => {}}
          onNewConversation={() => {}}
          onSearch={() => {}}
          onPinTask={() => {}}
          onUnpinTask={() => {}}
          onRenameTask={() => {}}
          onDeleteTask={() => {}}
          onRetryBootstrap={() => {}}
          onRequestWorkspaceTasks={() => {}}
          onRequestPinnedTasks={() => {}}
          onRequestRecentTasks={() => {}}
          onResetRecentTaskPage={onResetRecentTaskPage}
          onRequestTaskRuns={() => {}}
        />,
      );
    });
    try {
      const heading = container.querySelector('[data-conversation-sidebar-heading="recent"]') as HTMLElement;
      expect(heading.getAttribute('aria-expanded')).toBe('true');
      act(() => {
        heading.click();
      });
      expect(heading.getAttribute('aria-expanded')).toBe('false');
      expect(onResetRecentTaskPage).toHaveBeenCalledTimes(1);
    } finally {
      act(() => root.unmount());
    }

    // After the reset the page is not-loaded again: reopening re-requests the
    // first page instead of showing everything paged through last time.
    const onRequestRecentTasks = vi.fn();
    const resetVm = recentSidebar('cursor-2');
    resetVm.recentTasks = [];
    resetVm.recentTaskPage = { status: 'not-loaded', nextCursor: null };
    const container2 = document.createElement('div');
    document.body.append(container2);
    const root2: Root = createRoot(container2);
    act(() => {
      root2.render(
        <ConversationSidebar
          vm={resetVm}
          active={{ kind: 'conversation-home' }}
          onSelect={() => {}}
          onNewConversation={() => {}}
          onSearch={() => {}}
          onPinTask={() => {}}
          onUnpinTask={() => {}}
          onRenameTask={() => {}}
          onDeleteTask={() => {}}
          onRetryBootstrap={() => {}}
          onRequestWorkspaceTasks={() => {}}
          onRequestPinnedTasks={() => {}}
          onRequestRecentTasks={onRequestRecentTasks}
          onRequestTaskRuns={() => {}}
        />,
      );
    });
    try {
      const heading = container2.querySelector('[data-conversation-sidebar-heading="recent"]') as HTMLElement;
      expect(heading.getAttribute('aria-expanded')).toBe('true');
      expect(onRequestRecentTasks).not.toHaveBeenCalled();
      act(() => {
        heading.click();
      });
      expect(heading.getAttribute('aria-expanded')).toBe('false');
      act(() => {
        heading.click();
      });
      expect(onRequestRecentTasks).toHaveBeenCalledWith();
    } finally {
      act(() => root2.unmount());
    }
  });

  it('keeps the workspace load-more button hidden once the workspace list is exhausted', async () => {
    const vm = recentSidebar(null);
    vm.workspaces = [{ projectId: 'project-a', workspacePath: '/a', name: 'Workspace A' }];
    vm.lastActiveWorkspaceId = 'project-a';
    vm.preferences = { 'workspaces.collapsed': false };
    vm.tasksByWorkspace = {
      'project-a': [{
        projectId: 'project-a', taskId: 'task-1', taskUuid: 'uuid-1', title: 'Only', autoTitle: false,
        runMode: 'direct', runs: [], runHistoryStatus: 'ready', runsNextCursor: null, pinned: false,
      }],
    };
    vm.workspaceTaskPages = { 'project-a': { status: 'ready', nextCursor: null } };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    await act(async () => {
      root.render(
        <ConversationSidebar
          vm={vm}
          active={{ kind: 'conversation-home' }}
          onSelect={() => {}}
          onNewConversation={() => {}}
          onSearch={() => {}}
          onPinTask={() => {}}
          onUnpinTask={() => {}}
          onRenameTask={() => {}}
          onDeleteTask={() => {}}
          onRetryBootstrap={() => {}}
          onRequestWorkspaceTasks={() => {}}
          onRequestPinnedTasks={() => {}}
          onRequestRecentTasks={() => {}}
          onRequestTaskRuns={() => {}}
        />,
      );
    });
    try {
      // The auto-expanded workspace renders its rows; with the page exhausted
      // the load-more button may not appear (and no sentinel exists at all —
      // scrolling never pages a navigation tree).
      expect(container.querySelector('[data-conversation-workspace-group="project-a"] [data-task-id="task-1"]')).toBeTruthy();
      expect(container.textContent).not.toContain('conversation.sidebar.loadMoreConversations');
      expect(container.querySelector('[data-load-more-sentinel]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
