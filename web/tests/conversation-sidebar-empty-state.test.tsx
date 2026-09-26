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
import {
  applyConversationSidebarBootstrap,
  applyConversationTaskPage,
  createEmptyConversationSidebar,
  removeConversationSidebarTask,
} from '@/lib/conversation-sidebar-loading';
import { applyConversationTaskSnapshot } from '@/lib/conversation-task-state';
import type { ConversationSidebarVm, ConversationTaskRowVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const EMPTY_TEXT = 'conversation.noConversations';

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

function task(taskId: string): ConversationTaskRowVm {
  return {
    projectId: 'project-a',
    taskId,
    taskUuid: `uuid-${taskId}`,
    title: `Conversation ${taskId}`,
    autoTitle: false,
    runMode: 'direct',
    latestRun: null,
    runs: [],
    runHistoryStatus: 'not-loaded',
    runsNextCursor: null,
  };
}

function loadedWorkspace(tasks: ConversationTaskRowVm[]) {
  const sidebar = applyConversationSidebarBootstrap(createEmptyConversationSidebar(), {
    workspaces: [{ projectId: 'project-a', workspacePath: 'D:/A', name: 'A' }],
    pinRefs: [],
    lastActiveWorkspaceId: 'project-a',
    preferences: {},
  });
  return applyConversationTaskPage(sidebar, { projectId: 'project-a', tasks, nextCursor: null, errors: [] }, false);
}

async function renderedText(vm: ConversationSidebarVm) {
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => root.render(<ConversationSidebar {...callbacks} vm={vm} active={{ kind: 'conversation-home' }} />));
  const text = container.textContent ?? '';
  await act(async () => root.unmount());
  return text;
}

afterEach(() => {
  document.body.replaceChildren();
});

describe('ConversationSidebar workspace empty state', () => {
  it('hides the empty hint after a conversation is created in a loaded empty workspace', async () => {
    const empty = loadedWorkspace([]);
    expect(await renderedText(empty)).toContain(EMPTY_TEXT);

    const created = applyConversationTaskSnapshot(empty, task('task-new'));
    const text = await renderedText(created);
    expect(text).toContain('Conversation task-new');
    expect(text).not.toContain(EMPTY_TEXT);
  });

  it('shows the empty hint after the last conversation is removed', async () => {
    const loaded = loadedWorkspace([task('task-only')]);
    expect(await renderedText(loaded)).not.toContain(EMPTY_TEXT);

    const removed = removeConversationSidebarTask(loaded, 'project-a', 'task-only');
    expect(await renderedText(removed)).toContain(EMPTY_TEXT);
  });

  it('does not show the empty hint while more pages remain', async () => {
    const sidebar = applyConversationSidebarBootstrap(createEmptyConversationSidebar(), {
      workspaces: [{ projectId: 'project-a', workspacePath: 'D:/A', name: 'A' }],
      pinRefs: [],
      lastActiveWorkspaceId: 'project-a',
      preferences: {},
    });
    const paged = applyConversationTaskPage(sidebar, { projectId: 'project-a', tasks: [], nextCursor: 'cursor-2', errors: [] }, false);
    expect(await renderedText(paged)).not.toContain(EMPTY_TEXT);
  });
});
