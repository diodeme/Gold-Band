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
  onSelectTask: () => {},
  onSelectRun: () => {},
  onPinTask: () => {},
  onUnpinTask: () => {},
  onRenameTask: () => {},
  onDeleteTask: () => {},
};

function sidebarVm(): ConversationSidebarVm {
  return {
    workspaces: [
      { projectId: 'workspace-a', workspacePath: 'D:\\workspace-a', name: 'Workspace A' },
      { projectId: 'workspace-b', workspacePath: 'D:\\workspace-b', name: 'Workspace B' },
    ],
    pinnedTasks: [],
    tasksByWorkspace: { 'workspace-a': [], 'workspace-b': [] },
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
