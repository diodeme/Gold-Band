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
const chatMocks = vi.hoisted(() => ({
  render: vi.fn(() => null),
}));

vi.mock('@/components/acp/ACPChatDialog', () => ({
  ACPChatDialog: chatMocks.render,
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

  it('passes the selected dynamic session worktree to the composer instead of the run worktree', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const selectedKey = 'round-001/ai-dynamic/attempt-001/goodbye-worker/attempt-001';
    const leaf = {
      roundId: 'round-001',
      nodeId: 'goodbye-worker',
      attemptId: 'attempt-001',
      outerNodeId: 'ai-dynamic',
      outerAttemptId: 'attempt-001',
      pathLabel: selectedKey,
      status: 'completed',
      current: false,
      manualCheckPending: false,
      sessionEstablished: true,
      artifactCount: 0,
      attachmentCount: 0,
    };
    const run = {
      projectId: 'project-1',
      taskId: 'task-028',
      runId: 'run-001',
      runStatus: 'completed',
      runMode: 'auto',
      activeSessions: [],
      inputAttachments: [],
      selectedSession: {
        roundId: leaf.roundId,
        nodeId: leaf.nodeId,
        attemptId: leaf.attemptId,
        outerNodeId: leaf.outerNodeId,
        outerAttemptId: leaf.outerAttemptId,
        provider: 'codex-acp',
        status: 'completed',
        restored: true,
        worktreePath: 'D:/repo/.gold-band/worktrees/child',
        events: [],
        eventPage: { loadedCount: 0, total: 0, hasOlder: false, hasNewer: false },
        pendingPermissions: [],
        pendingElicitations: [],
        diagnostics: { rawFrameCount: 0, eventCount: 0, errorCount: 0 },
      },
      sessionTree: {
        selectedSessionKey: selectedKey,
        rounds: [{
          roundId: 'round-001',
          index: 1,
          label: 'Round 1',
          status: 'completed',
          nodes: [{
            nodeId: 'ai-dynamic',
            label: 'AUTO',
            nodeType: 'ai-dynamic',
            status: 'completed',
            attempts: [],
            outerNodes: [{
              nodeId: 'goodbye-worker',
              label: 'Goodbye worker',
              nodeType: 'worker',
              status: 'completed',
              attempts: [leaf],
            }],
          }],
        }],
      },
      workflowStatus: 'valid',
      workflowValid: true,
      workflowGraph: { nodes: [], edges: [] },
      resumable: false,
      worktree: { path: 'C:/GoldBand/worktrees/outer', branch: 'outer', forkCommit: 'abc123' },
    } as unknown as ConversationRunVm;

    await act(async () => {
      root.render(
        <ConversationRunPage
          run={run}
          taskTitle="AUTO run"
          appConfig={{ turnFiles: { cardPreviewLimit: 10 } } as AppConfigVm}
          agentRegistry={null}
          followMode="manual"
          onRerun={vi.fn()}
          onEditWorkflow={vi.fn()}
          onSelectSession={vi.fn()}
          onAutoFollowChange={vi.fn()}
          initialSessionTreeExpansion={{}}
          onSessionTreeExpansionChange={vi.fn()}
        />,
      );
    });

    expect(chatMocks.render).toHaveBeenCalledWith(
      expect.objectContaining({ worktreePath: 'D:/repo/.gold-band/worktrees/child' }),
      undefined,
    );

    await act(async () => root.unmount());
  });
});
