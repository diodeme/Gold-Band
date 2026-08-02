/** @vitest-environment jsdom */

import React, { act, useEffect } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/lib/conversation-event-router', async () => {
  const actual = await vi.importActual<typeof import('@/lib/conversation-event-router')>('@/lib/conversation-event-router');
  return {
    ...actual,
    useConversationBranchLiveSnapshot: () => ({ revision: 0, contentRevision: 0, status: null, attention: false }),
  };
});

vi.mock('@/components/workspace/AgentConversationPanel', () => ({
  AgentConversationPanel: ({ resource }: { resource: { locator: { branchId: string } } }) => (
    <div data-conversation-viewport="true" data-rendered-agent-branch={resource.locator.branchId} />
  ),
}));

import { ACPMessageList, buildAcpTimelineProjection } from '@/components/acp/ACPChatDialog';
import { RightWorkspaceDock } from '@/components/workspace/RightWorkspaceDock';
import {
  agentTranscriptResourceKey,
  RightWorkspaceProvider,
  useRightWorkspace,
  type AgentTranscriptLocator,
} from '@/components/workspace/right-workspace-context';
import type { AcpSessionVm, AcpUiEventVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const locator = (branchId: string): AgentTranscriptLocator => ({
  projectId: 'project-1',
  taskId: 'task-1',
  runId: 'run-1',
  roundId: 'round-1',
  nodeId: 'node-1',
  attemptId: 'attempt-1',
  branchId,
});

function resource(branchId: string) {
  const branchLocator = locator(branchId);
  return {
    kind: 'agent-transcript' as const,
    key: agentTranscriptResourceKey(branchLocator),
    title: branchId,
    status: 'running',
    attention: false,
    locator: branchLocator,
  };
}

function SeedTabs({ branches }: { branches: string[] }) {
  const workspace = useRightWorkspace();
  useEffect(() => {
    for (const branch of branches) workspace.openResource(resource(branch));
  }, [branches, workspace.openResource]);
  return null;
}

function WorkspaceProbe() {
  const workspace = useRightWorkspace();
  return (
    <output
      data-workspace-tab-count={workspace.tabs.length}
      data-workspace-active-tab={workspace.activeTabKey ?? ''}
    >
      {workspace.tabs.map((tab) => tab.kind === 'agent-transcript' ? tab.locator.branchId : tab.key).join(',')}
    </output>
  );
}

afterEach(() => {
  document.body.replaceChildren();
  vi.clearAllMocks();
});

describe('right workspace DOM lifecycle', () => {
  it('mounts a ConversationViewport only for the active Tab', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const branches = ['agent-a', 'agent-b'];
    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider>
            <SeedTabs branches={branches} />
            <RightWorkspaceDock />
          </RightWorkspaceProvider>,
        );
      });
      expect(container.querySelectorAll('[data-conversation-viewport="true"]')).toHaveLength(1);
      expect(container.querySelector('[data-rendered-agent-branch="agent-b"]')).not.toBeNull();

      const agentATab = Array.from(container.querySelectorAll('button'))
        .find((button) => button.textContent?.includes('agent-a'));
      await act(async () => {
        agentATab?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      expect(container.querySelectorAll('[data-conversation-viewport="true"]')).toHaveLength(1);
      expect(container.querySelector('[data-rendered-agent-branch="agent-a"]')).not.toBeNull();
      expect(container.querySelector('[data-rendered-agent-branch="agent-b"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('opens a nested Agent link as another workspace Tab', async () => {
    const childId = 'agent-child';
    const launch: AcpUiEventVm = {
      id: 'launch-child',
      seq: 2,
      timestamp: '2Z',
      kind: 'toolCall',
      sessionId: 'session-1',
      content: null,
      title: 'Agent child',
      toolCallId: 'provider-child',
      status: 'running',
      raw: {
        rawInput: { description: 'Nested child' },
        _meta: {
          goldBandConversation: {
            branchId: 'agent-parent',
            launchedAgentExecutionId: childId,
            toolName: 'Agent',
          },
        },
      },
    };
    const projectionVm: NonNullable<AcpSessionVm['timelineProjection']> = {
      todoEntries: [],
      agents: [{
        agentExecutionId: childId,
        parentAgentExecutionId: 'agent-parent',
        executionStatus: 'running',
        eventCount: 1,
        toolCallCount: 0,
        readFileCount: 0,
        writtenFileCount: 0,
        hasAttention: false,
        description: 'Nested child',
        todoEntries: [],
      }],
    };
    const projection = buildAcpTimelineProjection([launch], 'running', projectionVm);
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const parentBranches = ['agent-parent'];
    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider>
            <SeedTabs branches={parentBranches} />
            <ACPMessageList
              timeline={projection.timeline}
              sessionStatus="running"
              sending={false}
              branchLocator={locator('agent-parent')}
            />
            <WorkspaceProbe />
          </RightWorkspaceProvider>,
        );
      });
      const link = container.querySelector<HTMLButtonElement>(`[data-agent-link-branch-id="${childId}"]`);
      expect(link).not.toBeNull();
      await act(async () => {
        link?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      const probe = container.querySelector('output');
      expect(probe?.getAttribute('data-workspace-tab-count')).toBe('2');
      expect(probe?.textContent).toBe('agent-parent,agent-child');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
