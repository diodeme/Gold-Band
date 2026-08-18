/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const workspaceMocks = vi.hoisted(() => ({
  registerResourceRenderer: vi.fn(() => vi.fn()),
  registerResourceCloseResolver: vi.fn(() => vi.fn()),
  setConversationDirectoryEntry: vi.fn(),
  openResource: vi.fn(),
}));
const headerMocks = vi.hoisted(() => ({
  render: vi.fn(() => null),
}));

vi.mock('@/components/acp/ACPChatDialog', () => ({
  ACPChatDialog: () => null,
  createAcpEventWindowCacheKey: vi.fn(() => 'event-window'),
  hasHydratedAcpSessionContent: vi.fn(() => false),
}));
vi.mock('@/components/conversation/ConversationRunHeader', () => ({
  ConversationRunHeader: headerMocks.render,
}));
vi.mock('@/components/conversation/ConversationSessionSwitcher', () => ({
  ConversationSessionSwitcher: () => null,
}));
vi.mock('@/components/theme/ThemeAssetsContext', () => ({
  useThemeWallpaperSurface: vi.fn(),
}));
vi.mock('@/components/workspace/ConversationRunWorkspaceResourcePanel', () => ({
  ConversationRunWorkspaceResourcePanel: () => null,
  confirmCloseConversationRunWorkspaceResource: vi.fn(() => true),
}));
vi.mock('@/components/workspace/right-workspace-context', () => ({
  conversationRunWorkspaceResourceKey: vi.fn(() => 'resource-key'),
  useRightWorkspace: () => ({
    scopeKey: null,
    registerResourceRenderer: workspaceMocks.registerResourceRenderer,
    registerResourceCloseResolver: workspaceMocks.registerResourceCloseResolver,
    setConversationDirectoryEntry: workspaceMocks.setConversationDirectoryEntry,
    openResource: workspaceMocks.openResource,
  }),
}));

import { ConversationRunPage } from '@/pages/ConversationRunPage';
import type { AgentRegistryVm, AppConfigVm, ConversationRunVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.innerHTML = '';
  vi.clearAllMocks();
});

describe('ConversationRunPage follow mode reentry', () => {
  it('does not re-enable auto-follow when a manually browsed run remounts', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onAutoFollowChange = vi.fn();
    const run = {
      projectId: 'project-1',
      taskId: 'task-1',
      runId: 'run-1',
      runStatus: 'completed',
      runMode: 'workflow',
      activeSessions: [],
      selectedSession: null,
      sessionTree: { rounds: [], selectedSessionKey: null },
      workflowGraph: { nodes: [], edges: [] },
    } as unknown as ConversationRunVm;

    await act(async () => {
      root.render(
        <ConversationRunPage
          run={run}
          taskTitle="Run 1"
          appConfig={{} as AppConfigVm}
          agentRegistry={null as AgentRegistryVm | null}
          followMode="manual"
          onRerun={vi.fn()}
          onEditWorkflow={vi.fn()}
          onSelectSession={vi.fn()}
          onAutoFollowChange={onAutoFollowChange}
          initialSessionTreeExpansion={{}}
          onSessionTreeExpansionChange={vi.fn()}
        />,
      );
    });

    expect(onAutoFollowChange).not.toHaveBeenCalled();
    expect(headerMocks.render).toHaveBeenCalledWith(
      expect.objectContaining({ taskTitle: 'Run 1' }),
      undefined,
    );

    await act(async () => root.unmount());
  });
});
