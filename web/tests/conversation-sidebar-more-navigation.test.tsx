/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const labels: Record<string, string> = {
  'conversation.sidebar.more': '更多',
  'conversation.sidebar.multicaTaskManagement': '需求管理',
  'scheduled.management.title': '定时任务',
};

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => labels[key] ?? key }),
}));

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, saveConversationPreference: vi.fn().mockResolvedValue(undefined) };
});

vi.mock('@/components/ui/scroll-area', () => ({
  ScrollArea: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

import { ConversationSidebar } from '@/components/conversation/ConversationSidebar';
import type { ConversationPage, ConversationSidebarVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const sidebarVm: ConversationSidebarVm = {
  loadStatus: 'ready',
  workspaces: [],
  pinRefs: [],
  pinnedTasks: [],
  pinnedTaskPage: { status: 'ready-empty', nextCursor: null },
  tasksByWorkspace: {},
  workspaceTaskPages: {},
  lastActiveWorkspaceId: null,
};

const callbacks = {
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

function findButton(container: HTMLElement, label: string) {
  return [...container.querySelectorAll<HTMLButtonElement>('button')]
    .find((button) => button.textContent?.trim() === label);
}

afterEach(() => {
  document.body.replaceChildren();
});

describe('ConversationSidebar more navigation', () => {
  it('keeps low-frequency management pages collapsed until the user expands them', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onSelect = vi.fn();

    try {
      await act(async () => {
        root.render(
          <ConversationSidebar
            {...callbacks}
            vm={sidebarVm}
            active={{ kind: 'conversation-home' }}
            onSelect={onSelect}
          />,
        );
      });

      const moreButton = findButton(container, '更多');
      expect(moreButton).toBeDefined();
      expect(moreButton?.getAttribute('aria-expanded')).toBe('false');
      expect(findButton(container, '需求管理')).toBeUndefined();
      expect(findButton(container, '定时任务')).toBeUndefined();

      await act(async () => moreButton?.click());

      expect(moreButton?.getAttribute('aria-expanded')).toBe('true');
      const moreContent = container.querySelector<HTMLElement>('[data-conversation-sidebar-more-content]');
      expect(moreContent).not.toBeNull();
      expect(moreContent?.className).not.toMatch(/\bpl-/u);
      const requirementsButton = findButton(container, '需求管理');
      expect(requirementsButton).toBeDefined();
      expect(findButton(container, '定时任务')).toBeDefined();

      await act(async () => requirementsButton?.click());
      expect(onSelect).toHaveBeenCalledWith({ kind: 'multica-tasks' });
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('reveals the active child when route navigation enters the group', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const render = async (active: ConversationPage) => {
      await act(async () => {
        root.render(
          <ConversationSidebar
            {...callbacks}
            vm={sidebarVm}
            active={active}
            onSelect={() => {}}
          />,
        );
      });
    };

    try {
      await render({ kind: 'conversation-home' });
      expect(findButton(container, '更多')?.getAttribute('aria-expanded')).toBe('false');

      await render({ kind: 'multica-tasks' });
      expect(findButton(container, '更多')?.getAttribute('aria-expanded')).toBe('true');
      expect(findButton(container, '需求管理')).toBeDefined();

      await render({ kind: 'scheduled-task-detail', projectId: 'project-a', scheduledTaskId: 'schedule-a' });
      expect(findButton(container, '更多')?.getAttribute('aria-expanded')).toBe('true');
      expect(findButton(container, '定时任务')).toBeDefined();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
