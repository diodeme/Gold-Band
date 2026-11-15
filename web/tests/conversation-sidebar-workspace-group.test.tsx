/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, saveConversationPreference: vi.fn().mockResolvedValue(undefined) };
});

vi.mock('@/components/ui/scroll-area', () => ({
  ScrollArea: ({ children, viewportRef, ...props }: {
    children: React.ReactNode;
    viewportRef?: React.RefObject<HTMLDivElement | null>;
  } & Record<string, unknown>) => (
    <div ref={viewportRef} {...props}>{children}</div>
  ),
}));

import { ConversationSidebar } from '@/components/conversation/ConversationSidebar';
import { saveConversationPreference } from '@/api';
import type { ConversationSidebarVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const callbacks = {
  onSelect: () => {},
  onNewConversation: () => {},
  onSearch: () => {},
  onPinTask: () => {},
  onUnpinTask: () => {},
  onRenameTask: () => {},
  onDeleteTask: () => {},
  onRetryBootstrap: () => {},
  onRequestWorkspaceTasks: () => {},
  onRequestPinnedTasks: () => {},
  onRequestRecentTasks: () => {},
  onRequestTaskRuns: () => {},
};

function sidebarVm(preferences?: Record<string, boolean>): ConversationSidebarVm {
  return {
    loadStatus: 'ready',
    workspaces: [
      { projectId: 'workspace-a', workspacePath: 'D:\\workspace-a', name: 'Workspace A' },
      { projectId: 'workspace-b', workspacePath: 'D:\\workspace-b', name: 'Workspace B' },
    ],
    pinRefs: [],
    pinnedTasks: [],
    pinnedTaskPage: { status: 'ready', nextCursor: null },
    recentTasks: [],
    recentTaskPage: { status: 'not-loaded', nextCursor: null },
    tasksByWorkspace: { 'workspace-a': [], 'workspace-b': [] },
    workspaceTaskPages: {
      'workspace-a': { status: 'ready', nextCursor: null },
      'workspace-b': { status: 'ready', nextCursor: null },
    },
    lastActiveWorkspaceId: 'workspace-a',
    preferences,
  };
}

function mount(vm: ConversationSidebarVm, props: Record<string, unknown> = {}) {
  const container = document.createElement('div');
  document.body.append(container);
  const root: Root = createRoot(container);
  act(() => {
    root.render(
      <ConversationSidebar
        {...callbacks}
        vm={vm}
        active={{ kind: 'conversation-home' }}
        {...props}
      />,
    );
  });
  return { container, root };
}

beforeEach(() => {
  vi.mocked(saveConversationPreference).mockClear();
});

afterEach(() => {
  document.body.replaceChildren();
});

describe('ConversationSidebar workspaces group', () => {
  it('collapses by default and reveals workspace rows only after expanding', () => {
    const { container, root } = mount(sidebarVm());
    try {
      const groupHeading = container.querySelector('[data-conversation-sidebar-heading="workspaces"]')!;
      expect(groupHeading.getAttribute('aria-expanded')).toBe('false');
      // Collapsed group hides the second level entirely.
      expect(container.querySelector('[data-conversation-workspace-id="workspace-a"]')).toBeNull();

      act(() => {
        groupHeading.click();
      });
      expect(groupHeading.getAttribute('aria-expanded')).toBe('true');
      expect(container.querySelector('[data-conversation-workspace-id="workspace-a"]')).toBeTruthy();
      expect(container.querySelector('[data-conversation-workspace-id="workspace-b"]')).toBeTruthy();
      expect(saveConversationPreference).toHaveBeenCalledWith('workspaces.collapsed', false);
    } finally {
      act(() => root.unmount());
    }
  });

  it('honors a persisted expanded preference on mount', () => {
    const { container, root } = mount(sidebarVm({ 'workspaces.collapsed': false }));
    try {
      expect(container.querySelector('[data-conversation-sidebar-heading="workspaces"]')!.getAttribute('aria-expanded')).toBe('true');
      expect(container.querySelector('[data-conversation-workspace-id="workspace-a"]')).toBeTruthy();
    } finally {
      act(() => root.unmount());
    }
  });

  it('completes a reveal against an expanded workspace hidden by the collapsed group', () => {
    // Regression: the reveal first opens the group in a sibling layout
    // effect; the scroll pass that follows must re-run once the group opens,
    // find the target row and settle the reveal (onRevealHandled). Before the
    // fix the effect's dependencies missed the group state, the row never
    // appeared and the pending reveal leaked.
    const taskB = {
      projectId: 'workspace-b', taskId: 'task-001', taskUuid: 'uuid-001',
      title: 'Target', autoTitle: false, runMode: 'direct', runs: [],
      runHistoryStatus: 'ready' as const, runsNextCursor: null, pinned: false,
    };
    const baseVm = sidebarVm({ 'workspaces.collapsed': false });
    baseVm.tasksByWorkspace = { 'workspace-a': [], 'workspace-b': [taskB] };
    const container = document.createElement('div');
    document.body.append(container);
    const root: Root = createRoot(container);
    const onRevealHandled = vi.fn();
    const render = (request: Record<string, unknown> | null, active: Record<string, unknown>) => act(() => {
      root.render(
        <ConversationSidebar
          {...callbacks}
          vm={baseVm}
          active={active as never}
          workspaceRevealRequest={request as never}
          onRevealHandled={onRevealHandled}
        />,
      );
    });
    try {
      render(null, { kind: 'conversation-home' });
      // Expand workspace-b, then collapse the first-level group.
      act(() => {
        container.querySelector('[data-conversation-workspace-id="workspace-b"]')!.click();
      });
      act(() => {
        container.querySelector('[data-conversation-sidebar-heading="workspaces"]')!.click();
      });
      expect(container.querySelector('[data-conversation-workspace-id="workspace-b"]')).toBeNull();

      render(
        { projectId: 'workspace-b', requestId: 7, taskId: 'task-001', taskUuid: 'uuid-001' },
        { kind: 'conversation-run', projectId: 'workspace-b', taskId: 'task-001', runId: 'run-001' },
      );
      expect(container.querySelector('[data-conversation-workspace-id="workspace-b"]')).toBeTruthy();
      expect(onRevealHandled).toHaveBeenCalledWith(7);
    } finally {
      act(() => root.unmount());
      document.body.replaceChildren();
    }
  });

  it('does not re-collapse the group from a preferences echo after a reveal', () => {
    // Regression: re-applied bootstrap swaps the preferences object identity;
    // echoing our own last-saved value must not override the transient reveal
    // expansion. Only a changed (external) value may re-apply.
    const vmA = sidebarVm({ 'workspaces.collapsed': true });
    const container = document.createElement('div');
    document.body.append(container);
    const root: Root = createRoot(container);
    const render = (vm: ConversationSidebarVm, request: Record<string, unknown> | null) => act(() => {
      root.render(
        <ConversationSidebar
          {...callbacks}
          vm={vm}
          active={{ kind: 'conversation-home' }}
          workspaceRevealRequest={request as never}
        />,
      );
    });
    try {
      render(vmA, null);
      expect(container.querySelector('[data-conversation-sidebar-heading="workspaces"]')!.getAttribute('aria-expanded')).toBe('false');

      // Reveal opens the group for this turn without writing the preference.
      render(vmA, { projectId: 'workspace-b', requestId: 1 });
      expect(container.querySelector('[data-conversation-sidebar-heading="workspaces"]')!.getAttribute('aria-expanded')).toBe('true');

      // Bootstrap re-applied: fresh object identity, same persisted value.
      const vmB = sidebarVm({ 'workspaces.collapsed': true });
      render(vmB, null);
      expect(container.querySelector('[data-conversation-sidebar-heading="workspaces"]')!.getAttribute('aria-expanded')).toBe('true');
      expect(vi.mocked(saveConversationPreference)).not.toHaveBeenCalled();

      // A genuinely different persisted value still applies.
      const vmC = sidebarVm({ 'workspaces.collapsed': false });
      render(vmC, null);
      expect(container.querySelector('[data-conversation-sidebar-heading="workspaces"]')!.getAttribute('aria-expanded')).toBe('true');
    } finally {
      act(() => root.unmount());
      document.body.replaceChildren();
    }
  });

  it('preserves per-workspace expansion when the group collapses and re-expands', () => {
    // The group toggle is a visual zoom-out, not a navigation reset: collapsing
    // the first level unmounts the second level without touching its state, so
    // reopening shows each workspace exactly as expanded before (data-loading
    // rule: view changes must not replace the user's expansion state).
    const { container, root } = mount(sidebarVm({ 'workspaces.collapsed': false }));
    const groupHeading = () => container.querySelector('[data-conversation-sidebar-heading="workspaces"]')!;
    try {
      // workspace-a auto-expands via the initial reconcile; open workspace-b too.
      const wsB = container.querySelector('[data-conversation-workspace-id="workspace-b"]')!;
      act(() => {
        wsB.click();
      });
      expect(wsB.getAttribute('aria-expanded')).toBe('true');

      act(() => {
        groupHeading().click();
      });
      expect(groupHeading().getAttribute('aria-expanded')).toBe('false');
      expect(container.querySelector('[data-conversation-workspace-id="workspace-b"]')).toBeNull();

      act(() => {
        groupHeading().click();
      });
      const wsBAfter = container.querySelector('[data-conversation-workspace-id="workspace-b"]')!;
      expect(wsBAfter.getAttribute('aria-expanded')).toBe('true');
    } finally {
      act(() => root.unmount());
    }
  });

  it('owns the add-workspace entry at the first level with no tail button', () => {
    const onAddWorkspace = vi.fn();
    const { container, root } = mount(sidebarVm(), { onAddWorkspace });
    try {
      const groupHeading = container.querySelector('[data-conversation-sidebar-heading="workspaces"]')!;
      // Exactly one add-workspace control, living on the group header.
      const addButtons = container.querySelectorAll('[aria-label="conversation.sidebar.addWorkspace"]');
      expect(addButtons).toHaveLength(1);
      expect(addButtons[0].closest('[data-conversation-sidebar-region="workspaces"]')).toBeTruthy();
      act(() => {
        (addButtons[0] as HTMLElement).click();
      });
      expect(onAddWorkspace).toHaveBeenCalledTimes(1);
      // The old full-width tail button (text label) is gone.
      expect([...container.querySelectorAll('button')].some((button) => button.textContent === 'conversation.sidebar.addWorkspace')).toBe(false);
    } finally {
      act(() => root.unmount());
    }
  });

  it('auto-expands for an explicit reveal request without overwriting the persisted preference', () => {
    const { container, root } = mount(sidebarVm());
    const rerender = (request: Record<string, unknown> | null) => act(() => {
      root.render(
        <ConversationSidebar
          {...callbacks}
          vm={sidebarVm()}
          active={{ kind: 'conversation-home' }}
          workspaceRevealRequest={request as never}
        />,
      );
    });
    try {
      expect(container.querySelector('[data-conversation-sidebar-heading="workspaces"]')!.getAttribute('aria-expanded')).toBe('false');
      rerender({ projectId: 'workspace-b', requestId: 1 });
      expect(container.querySelector('[data-conversation-sidebar-heading="workspaces"]')!.getAttribute('aria-expanded')).toBe('true');
      expect(container.querySelector('[data-conversation-workspace-id="workspace-b"]')).toBeTruthy();
      // Navigation intent expands for this turn only; the persisted collapse
      // preference must survive untouched.
      expect(saveConversationPreference).not.toHaveBeenCalled();
    } finally {
      act(() => root.unmount());
    }
  });

  it('collapsing a workspace or the group resets pages so reopening shows the default first page', () => {
    // 2026-11-06: fold-away is a natural reset gesture — anything paged beyond
    // the default page size is dropped, and the next expand re-requests page
    // one through the not-loaded path instead of replaying the deep pages.
    const onResetWorkspaceTaskPage = vi.fn();
    const onRequestWorkspaceTasks = vi.fn();
    const baseVm = sidebarVm({ 'workspaces.collapsed': false });
    const taskB = {
      projectId: 'workspace-b', taskId: 'task-001', taskUuid: 'uuid-001',
      title: 'Target', autoTitle: false, runMode: 'direct', runs: [],
      runHistoryStatus: 'ready' as const, runsNextCursor: null, pinned: false,
    };
    baseVm.tasksByWorkspace = { 'workspace-a': [], 'workspace-b': [taskB] };
    baseVm.workspaceTaskPages = {
      'workspace-a': { status: 'ready', nextCursor: null },
      'workspace-b': { status: 'ready', nextCursor: null },
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root: Root = createRoot(container);
    act(() => {
      root.render(
        <ConversationSidebar
          {...callbacks}
          onRequestWorkspaceTasks={onRequestWorkspaceTasks}
          onResetWorkspaceTaskPage={onResetWorkspaceTaskPage}
          vm={baseVm}
          active={{ kind: 'conversation-home' }}
        />,
      );
    });
    try {
      // workspace-a auto-expands via the initial reconcile; open workspace-b.
      act(() => {
        container.querySelector('[data-conversation-workspace-id="workspace-b"]')!.click();
      });
      expect(onResetWorkspaceTaskPage).not.toHaveBeenCalled();
      // Collapsing the workspace row resets only that workspace.
      act(() => {
        container.querySelector('[data-conversation-workspace-id="workspace-b"]')!.click();
      });
      expect(onResetWorkspaceTaskPage).toHaveBeenCalledTimes(1);
      expect(onResetWorkspaceTaskPage).toHaveBeenCalledWith('workspace-b');
      // Collapsing the group resets every still-expanded workspace.
      onResetWorkspaceTaskPage.mockClear();
      act(() => {
        container.querySelector('[data-conversation-sidebar-heading="workspaces"]')!.click();
      });
      expect(onResetWorkspaceTaskPage).toHaveBeenCalledTimes(1);
      expect(onResetWorkspaceTaskPage).toHaveBeenCalledWith('workspace-a');

      // Reopening the group (click, not a preference echo) restores
      // remembered-expanded workspaces whose pages were reset (not-loaded):
      // the default first page is re-requested instead of rendering an
      // expanded workspace with no rows and no load affordance.
      const resetVm = sidebarVm({ 'workspaces.collapsed': true });
      resetVm.tasksByWorkspace = { 'workspace-a': [], 'workspace-b': [] };
      resetVm.workspaceTaskPages = {
        'workspace-a': { status: 'not-loaded', nextCursor: null },
        'workspace-b': { status: 'not-loaded', nextCursor: null },
      };
      onRequestWorkspaceTasks.mockClear();
      act(() => {
        root.render(
          <ConversationSidebar
            {...callbacks}
            onRequestWorkspaceTasks={onRequestWorkspaceTasks}
            onResetWorkspaceTaskPage={onResetWorkspaceTaskPage}
            vm={resetVm}
            active={{ kind: 'conversation-home' }}
          />,
        );
      });
      expect(container.querySelector('[data-conversation-sidebar-heading="workspaces"]')!.getAttribute('aria-expanded')).toBe('false');
      act(() => {
        container.querySelector('[data-conversation-sidebar-heading="workspaces"]')!.click();
      });
      expect(onRequestWorkspaceTasks).toHaveBeenCalledWith('workspace-a');
    } finally {
      act(() => root.unmount());
      document.body.replaceChildren();
    }
  });
});
