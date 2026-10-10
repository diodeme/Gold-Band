/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/components/ui/scroll-area', () => ({
  ScrollArea: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

import { ConversationSidebar } from '@/components/conversation/ConversationSidebar';
import {
  conversationTaskActivityFromLifecycle,
  conversationTaskActivityFromUpdate,
} from '@/lib/conversation-sidebar-activity';
import type {
  ConversationAttemptLifecycleVm,
  ConversationSidebarVm,
  ConversationTaskActivityVm,
  DirectBackgroundControl,
} from '@/types';

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

function sidebarVm(activity: ConversationTaskActivityVm | null): ConversationSidebarVm {
  return {
    loadStatus: 'ready',
    workspaces: [{ projectId: 'workspace-a', workspacePath: 'D:\\workspace-a', name: 'Workspace A' }],
    pinRefs: [],
    pinnedTasks: [],
    pinnedTaskPage: { status: 'ready', nextCursor: null },
    tasksByWorkspace: {
      'workspace-a': [{
        projectId: 'workspace-a', taskId: 'task-a', taskUuid: 'uuid-a', title: 'Background task', autoTitle: false,
        runMode: 'direct', latestRun: null, runs: [], runHistoryStatus: 'ready', runsNextCursor: null, pinned: false,
        agentIdentity: { agentType: 'claude-acp', displayName: 'Claude', iconKey: 'claude' },
        lastActivityAt: new Date(Date.now() - 3 * 60_000).toISOString(),
        activity,
      }],
    },
    workspaceTaskPages: { 'workspace-a': { status: 'ready', nextCursor: null } },
    lastActiveWorkspaceId: 'workspace-a',
  };
}

const control = (activeTools: number, expiresInMs: number): DirectBackgroundControl => ({
  sessionId: 'session', connectionGeneration: 1, activeTools, expiresAtMs: Date.now() + expiresInMs,
});

function idleLifecycle(backgroundControl: DirectBackgroundControl | null): ConversationAttemptLifecycleVm {
  return {
    runtime: { status: 'completed', resumable: false, current: true, active: false, continuable: true, phase: 'terminal' },
    acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'completed', stopping: false },
    composer: { mode: 'normal', submitTarget: 'acp-prompt', processingKind: 'processing', statusKey: null, canStop: false, lockInput: false, backgroundControl },
  } as unknown as ConversationAttemptLifecycleVm;
}

afterEach(() => {
  vi.useRealTimers();
  document.body.replaceChildren();
});

describe('conversation sidebar activity indicator', () => {
  it('projects a busy background into task activity even when the prompt turn is idle', () => {
    const busy = control(1, 10_000);
    const activity = conversationTaskActivityFromLifecycle(idleLifecycle(busy));
    expect(activity).toEqual({ phase: 'background', stopping: false, backgroundControl: busy });
    expect(conversationTaskActivityFromLifecycle(idleLifecycle(null))).toBeNull();
    // The idle prompt lifecycle no longer clears a busy background sample.
    expect(conversationTaskActivityFromUpdate({ lifecycle: idleLifecycle(busy), activity } as never)).toEqual(activity);
    expect(conversationTaskActivityFromUpdate({ lifecycle: idleLifecycle(null), activity } as never)).toBeNull();
  });

  it('replaces the relative time with a running spinner while active and restores it afterwards', async () => {
    vi.useFakeTimers();
    const container = document.createElement('div'); document.body.append(container);
    const root = createRoot(container);
    const render = (activity: ConversationTaskActivityVm | null) => act(async () => root.render(
      <ConversationSidebar {...callbacks} vm={sidebarVm(activity)} active={{ kind: 'conversation-home' }} />,
    ));
    const spinner = () => container.querySelector('[role="status"][aria-label="status.running"] [data-acp-processing-spinner]');
    const time = () => container.textContent?.includes('3m') ?? false;
    try {
      await render({ phase: 'running', stopping: false });
      expect(spinner()).not.toBeNull();
      expect(time()).toBe(false);

      await render({ phase: 'background', stopping: false, backgroundControl: control(0, 10_000) });
      expect(spinner()).not.toBeNull();
      await act(async () => vi.advanceTimersByTime(10_001));
      expect(spinner()).toBeNull();
      expect(time()).toBe(true);

      await render(null);
      expect(spinner()).toBeNull();
      expect(time()).toBe(true);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
