/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, saveConversationPreference: vi.fn().mockResolvedValue(undefined) };
});

vi.mock('@/components/ui/scroll-area', () => ({
  ScrollArea: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

import { ConversationSidebar } from '@/components/conversation/ConversationSidebar';
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
  onRequestTaskRuns: () => {},
};

function sidebarVm(): ConversationSidebarVm {
  return {
    loadStatus: 'ready',
    workspaces: [
      { projectId: 'workspace-a', workspacePath: 'D:\\workspace-a', name: 'Workspace A' },
      { projectId: 'workspace-b', workspacePath: 'D:\\workspace-b', name: 'Workspace B' },
    ],
    pinRefs: [],
    pinnedTasks: [],
    pinnedTaskPage: { status: 'ready-empty', nextCursor: null },
    tasksByWorkspace: { 'workspace-a': [], 'workspace-b': [] },
    workspaceTaskPages: {
      'workspace-a': { status: 'ready-empty', nextCursor: null },
      'workspace-b': { status: 'ready-empty', nextCursor: null },
    },
    lastActiveWorkspaceId: 'workspace-a',
  };
}

function workspaceButton(container: HTMLElement, projectId: string) {
  return container.querySelector<HTMLButtonElement>(`[data-conversation-workspace-id="${projectId}"]`)!;
}

afterEach(() => {
  document.body.replaceChildren();
});

describe('ConversationSidebar workspace expansion intent', () => {
  it('navigates a pinned-only Direct task without loading its workspace task page first', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onSelect = vi.fn();
    const onRequestWorkspaceTasks = vi.fn();
    const vm = sidebarVm();
    vm.pinnedTasks = [{
      projectId: 'workspace-a',
      taskId: 'task-pinned',
      taskUuid: 'task-uuid-pinned',
      title: 'Pinned only conversation',
      autoTitle: false,
      runMode: 'direct',
      latestRun: {
        runId: 'run-007',
        status: 'completed',
        outcome: 'success',
        startedAt: '2026-08-31T08:00:00Z',
        updatedAt: '2026-08-31T08:01:00Z',
        resumable: true,
      },
      runs: [],
      runHistoryStatus: 'ready-empty',
      runsNextCursor: null,
      pinned: true,
      pinnedOrder: 0,
    }];
    vm.pinnedTaskPage = { status: 'ready', nextCursor: null };
    vm.tasksByWorkspace['workspace-a'] = [];
    vm.workspaceTaskPages['workspace-a'] = { status: 'not-loaded', nextCursor: null };

    try {
      await act(async () => {
        root.render(
          <ConversationSidebar
            {...callbacks}
            vm={vm}
            active={{ kind: 'conversation-home' }}
            defaultExpandedWorkspaceId="workspace-a"
            onSelect={onSelect}
            onRequestWorkspaceTasks={onRequestWorkspaceTasks}
          />,
        );
      });

      const taskTitle = [...container.querySelectorAll('span')]
        .find((element) => element.textContent === 'Pinned only conversation');
      expect(taskTitle).toBeDefined();

      await act(async () => {
        taskTitle?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });

      expect(onSelect).toHaveBeenCalledWith({
        kind: 'conversation-run',
        projectId: 'workspace-a',
        taskId: 'task-pinned',
        taskUuid: 'task-uuid-pinned',
        runId: 'run-007',
      });
      expect(onRequestWorkspaceTasks).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps workspace task and explicit run navigation semantics on the canonical page callback', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onSelect = vi.fn();
    const vm = sidebarVm();
    const latestRun = {
      runId: 'run-007',
      status: 'completed' as const,
      outcome: 'success' as const,
      startedAt: '2026-08-31T08:00:00Z',
      updatedAt: '2026-08-31T08:01:00Z',
      resumable: true,
    };
    vm.tasksByWorkspace['workspace-a'] = [{
      projectId: 'workspace-a',
      taskId: 'task-workspace',
      taskUuid: 'task-uuid-workspace',
      title: 'Workspace conversation',
      autoTitle: false,
      runMode: 'workflow',
      latestRun,
      runs: [{
        runId: 'run-006',
        status: 'paused',
        outcome: null,
        startedAt: '2026-08-31T07:00:00Z',
        updatedAt: '2026-08-31T07:01:00Z',
        resumable: true,
      }, latestRun],
      runHistoryStatus: 'ready',
      runsNextCursor: null,
      pinned: false,
      pinnedOrder: null,
    }];
    vm.workspaceTaskPages['workspace-a'] = { status: 'ready', nextCursor: null };

    try {
      await act(async () => {
        root.render(
          <ConversationSidebar
            {...callbacks}
            vm={vm}
            active={{ kind: 'conversation-home' }}
            defaultExpandedWorkspaceId="workspace-a"
            onSelect={onSelect}
          />,
        );
      });

      const taskTitle = [...container.querySelectorAll('span')]
        .find((element) => element.textContent === 'Workspace conversation');
      await act(async () => {
        taskTitle?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      expect(onSelect).toHaveBeenLastCalledWith({
        kind: 'conversation-run',
        projectId: 'workspace-a',
        taskId: 'task-workspace',
        taskUuid: 'task-uuid-workspace',
        runId: 'run-007',
      });

      const historicalRun = [...container.querySelectorAll('span')]
        .find((element) => element.textContent === 'run-006');
      expect(historicalRun).toBeDefined();
      await act(async () => {
        historicalRun?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      expect(onSelect).toHaveBeenLastCalledWith({
        kind: 'conversation-run',
        projectId: 'workspace-a',
        taskId: 'task-workspace',
        taskUuid: 'task-uuid-workspace',
        runId: 'run-006',
      });
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not project a not-yet-loaded workspace as an empty conversation list', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const vm = sidebarVm();
    vm.workspaceTaskPages['workspace-a'] = { status: 'loading', nextCursor: null };

    try {
      await act(async () => {
        root.render(
          <ConversationSidebar
            {...callbacks}
            vm={vm}
            active={{ kind: 'conversation-home' }}
            defaultExpandedWorkspaceId="workspace-a"
          />,
        );
      });

      expect(container.textContent).toContain('conversation.sidebar.loadingConversations');
      expect(container.textContent).not.toContain('conversation.noConversations');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('exposes the absolute workspace path from the heading by hover or keyboard focus', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <ConversationSidebar
            {...callbacks}
            vm={sidebarVm()}
            active={{ kind: 'conversation-home' }}
            defaultExpandedWorkspaceId="workspace-a"
          />,
        );
      });
      await act(async () => workspaceButton(container, 'workspace-a').focus());
      expect(document.body.querySelector('[data-slot="tooltip-content"]')?.textContent).toBe('D:\\workspace-a');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps workspace groups on the compact sidebar spacing token', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <ConversationSidebar
            {...callbacks}
            vm={sidebarVm()}
            active={{ kind: 'conversation-home' }}
            defaultExpandedWorkspaceId="workspace-a"
          />,
        );
      });

      const workspaceGroups = container.querySelectorAll<HTMLElement>('[data-conversation-workspace-group]');
      expect(workspaceGroups).toHaveLength(2);
      expect([...workspaceGroups].every((group) => group.classList.contains('mb-2'))).toBe(true);
      expect([...workspaceGroups].some((group) => group.classList.contains('mb-4'))).toBe(false);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('preserves a manual collapse across draft-target changes and fresh sidebar snapshots', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <ConversationSidebar
            {...callbacks}
            vm={sidebarVm()}
            active={{ kind: 'conversation-home' }}
            defaultExpandedWorkspaceId="workspace-a"
          />,
        );
      });
      expect(workspaceButton(container, 'workspace-a').getAttribute('aria-expanded')).toBe('true');

      await act(async () => {
        workspaceButton(container, 'workspace-a').click();
      });
      expect(workspaceButton(container, 'workspace-a').getAttribute('aria-expanded')).toBe('false');

      await act(async () => {
        root.render(
          <ConversationSidebar
            {...callbacks}
            vm={sidebarVm()}
            active={{ kind: 'conversation-home' }}
            defaultExpandedWorkspaceId="workspace-b"
          />,
        );
      });

      expect(workspaceButton(container, 'workspace-a').getAttribute('aria-expanded')).toBe('false');
      expect(workspaceButton(container, 'workspace-b').getAttribute('aria-expanded')).toBe('false');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('expands only when a new explicit reveal request arrives', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <ConversationSidebar
            {...callbacks}
            vm={sidebarVm()}
            active={{ kind: 'conversation-home' }}
            defaultExpandedWorkspaceId="workspace-a"
          />,
        );
      });
      expect(workspaceButton(container, 'workspace-b').getAttribute('aria-expanded')).toBe('false');

      await act(async () => {
        root.render(
          <ConversationSidebar
            {...callbacks}
            vm={sidebarVm()}
            active={{ kind: 'conversation-run', projectId: 'workspace-b', taskId: 'task-b', runId: 'run-b' }}
            defaultExpandedWorkspaceId="workspace-b"
            workspaceRevealRequest={{ projectId: 'workspace-b', requestId: 1 }}
          />,
        );
      });
      expect(workspaceButton(container, 'workspace-b').getAttribute('aria-expanded')).toBe('true');

      await act(async () => {
        workspaceButton(container, 'workspace-b').click();
        root.render(
          <ConversationSidebar
            {...callbacks}
            vm={sidebarVm()}
            active={{ kind: 'conversation-run', projectId: 'workspace-b', taskId: 'task-b', runId: 'run-b' }}
            defaultExpandedWorkspaceId="workspace-b"
            workspaceRevealRequest={{ projectId: 'workspace-b', requestId: 1 }}
          />,
        );
      });
      expect(workspaceButton(container, 'workspace-b').getAttribute('aria-expanded')).toBe('false');

      await act(async () => {
        root.render(
          <ConversationSidebar
            {...callbacks}
            vm={sidebarVm()}
            active={{ kind: 'conversation-run', projectId: 'workspace-b', taskId: 'task-b', runId: 'run-c' }}
            defaultExpandedWorkspaceId="workspace-b"
            workspaceRevealRequest={{ projectId: 'workspace-b', requestId: 2 }}
          />,
        );
      });
      expect(workspaceButton(container, 'workspace-b').getAttribute('aria-expanded')).toBe('true');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
