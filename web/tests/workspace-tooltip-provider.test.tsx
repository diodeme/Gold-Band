/** @vitest-environment jsdom */

import React, { act, useEffect } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, saveConversationPreference: vi.fn().mockResolvedValue(undefined) };
});

vi.mock('@/components/AppTitleBar', () => ({
  AppTitleBar: () => <header data-testid="app-title-bar" />,
}));

vi.mock('@/components/conversation/ConversationSidebar', () => ({
  ConversationSidebar: () => <aside data-testid="conversation-sidebar" />,
}));

vi.mock('@/components/ui/resizable', () => ({
  ResizablePanelGroup: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  ResizablePanel: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  ResizableHandle: () => <div />,
}));

vi.mock('@/components/ui/sheet', () => ({
  Sheet: () => null,
  SheetContent: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  SheetTitle: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

vi.mock('@/lib/conversation-event-router', async () => {
  const actual = await vi.importActual<typeof import('@/lib/conversation-event-router')>('@/lib/conversation-event-router');
  return {
    ...actual,
    useConversationBranchLiveSnapshot: () => ({ revision: 0, contentRevision: 0, status: null, attention: false }),
  };
});

vi.mock('@/components/workspace/AgentConversationPanel', async () => {
  const { Tooltip, TooltipContent, TooltipTrigger } = await vi.importActual<typeof import('@/components/ui/tooltip')>('@/components/ui/tooltip');
  return {
    AgentConversationPanel: () => (
      <Tooltip>
        <TooltipTrigger asChild>
          <button type="button">Agent conversation action</button>
        </TooltipTrigger>
        <TooltipContent>Agent conversation tooltip</TooltipContent>
      </Tooltip>
    ),
  };
});

import { WorkspaceShell } from '@/components/workspace/WorkspaceShell';
import {
  agentTranscriptResourceKey,
  useRightWorkspace,
  type AgentTranscriptResource,
} from '@/components/workspace/right-workspace-context';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const resource: AgentTranscriptResource = {
  kind: 'agent-transcript',
  key: agentTranscriptResourceKey({
    projectId: 'project-1',
    taskId: 'task-1',
    runId: 'run-1',
    roundId: 'round-1',
    nodeId: 'node-1',
    attemptId: 'attempt-1',
    branchId: 'agent-1',
  }),
  title: 'Agent 1',
  status: 'running',
  attention: false,
  locator: {
    projectId: 'project-1',
    taskId: 'task-1',
    runId: 'run-1',
    roundId: 'round-1',
    nodeId: 'node-1',
    attemptId: 'attempt-1',
    branchId: 'agent-1',
  },
};

function OpenAgentWorkspace() {
  const workspace = useRightWorkspace();
  useEffect(() => workspace.openResource(resource), [workspace.openResource]);
  return null;
}

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe('WorkspaceShell tooltip boundary', () => {
  it('covers an Agent conversation opened in the right workspace', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <WorkspaceShell
            appName="Gold Band"
            windowFrameStyle="native-compositor"
            vm={{ workspaces: [], pinnedTasks: [], tasksByWorkspace: {}, preferences: null }}
            active={{ kind: 'conversation-home' }}
            sidebarCollapsed={false}
            onSelect={() => {}}
            onToggleSidebar={() => {}}
            onNewConversation={() => {}}
            onSearch={() => {}}
            onSelectTask={() => {}}
            onSelectRun={() => {}}
            onPinTask={() => {}}
            onUnpinTask={() => {}}
            onRenameTask={() => {}}
            onDeleteTask={() => {}}
          >
            <OpenAgentWorkspace />
          </WorkspaceShell>,
        );
      });

      expect(container.querySelector('[data-right-workspace-dock="true"]')).not.toBeNull();
      expect(container.querySelector('[data-slot="tooltip-trigger"]')?.textContent)
        .toBe('Agent conversation action');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
