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

afterEach(() => {
  document.body.replaceChildren();
});

describe('ConversationSidebar Direct terminal result dot', () => {
  it('overlays a semantic unread result on the Agent identity without replacing breathing activity', async () => {
    const vm: ConversationSidebarVm = {
      loadStatus: 'ready',
      workspaces: [{ projectId: 'project-001', workspacePath: 'D:/project', name: 'Project' }],
      pinRefs: [],
      pinnedTasks: [],
      pinnedTaskPage: { status: 'ready-empty', nextCursor: null },
      tasksByWorkspace: {
        'project-001': [{
          projectId: 'project-001',
          taskId: 'task-001',
          title: 'Direct conversation',
          autoTitle: false,
          runMode: 'direct',
          agentIdentity: { agentType: 'codex', displayName: 'Codex', iconKey: 'codex' },
          activity: { phase: 'running', stopping: false },
          unreadTerminalResult: {
            eventId: 'event-001',
            runId: 'run-001',
            kind: 'failed',
            occurredAt: '2026-08-18T10:00:00Z',
          },
          runs: [],
          runHistoryStatus: 'ready-empty',
          runsNextCursor: null,
          pinned: false,
        }],
      },
      workspaceTaskPages: { 'project-001': { status: 'ready', nextCursor: null } },
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <ConversationSidebar
            vm={vm}
            active={{ kind: 'conversation-home' }}
            defaultExpandedWorkspaceId="project-001"
            onSelect={() => {}}
            onNewConversation={() => {}}
            onSearch={() => {}}
            onSelectTask={() => {}}
            onSelectRun={() => {}}
            onPinTask={() => {}}
            onUnpinTask={() => {}}
            onRenameTask={() => {}}
            onDeleteTask={() => {}}
            onRetryBootstrap={() => {}}
            onRequestWorkspaceTasks={() => {}}
            onRequestPinnedTasks={() => {}}
            onRequestTaskRuns={() => {}}
          />,
        );
      });

      const identity = container.querySelector<HTMLElement>('[data-conversation-terminal-result="failed"]');
      expect(identity).not.toBeNull();
      expect(identity?.getAttribute('aria-label')).toContain('conversation.sidebar.terminalResult.failed');
      expect(identity?.querySelector('img')?.className).toContain('motion-safe:animate-pulse');
      const dot = identity?.querySelector<HTMLElement>('span[aria-hidden="true"]');
      expect(dot?.className).toContain('absolute');
      expect(dot?.className).toContain('-top-0.5');
      expect(dot?.className).toContain('-right-0.5');
      expect(dot?.className).toContain('bg-gold-danger');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
