import { beforeEach, describe, expect, it } from 'vitest';

import type { AcpSessionUpdatedEventVm } from '@/api/client';
import {
  applyConversationEventToBranchSnapshots,
  conversationEventMatchesAttempt,
  readConversationBranchLiveSnapshot,
  resetConversationEventRouterSnapshots,
} from '@/lib/conversation-event-router';
import type { AcpAgentExecutionVm, AcpSessionVm, AcpUiEventVm } from '@/types';

const locator = {
  projectId: 'project-1',
  taskId: 'task-1',
  runId: 'run-1',
  roundId: 'round-1',
  nodeId: 'node-1',
  attemptId: 'attempt-1',
};

function uiEvent(kind: string, status: string | null = null): AcpUiEventVm {
  return {
    id: `${kind}-1`,
    seq: 1,
    timestamp: '1Z',
    kind,
    sessionId: 'session-1',
    content: null,
    title: null,
    toolCallId: null,
    status,
    raw: null,
  };
}

function live(branchId: string, event: AcpUiEventVm): AcpSessionUpdatedEventVm {
  return { ...locator, branchId, event };
}

function sessionUpdate(status: string, agents: AcpAgentExecutionVm[]): AcpSessionUpdatedEventVm {
  const session: AcpSessionVm = {
    branchId: 'root',
    parentBranchId: null,
    readOnly: false,
    provider: 'test',
    status,
    restored: false,
    events: [],
    eventPage: {
      loadedCount: 0,
      total: 0,
      hasOlder: false,
      hasNewer: false,
    },
    timelineProjection: { agents, todoEntries: [] },
    pendingPermissions: [],
    diagnostics: { rawFrameCount: 0, eventCount: 0, errorCount: 0 },
  };
  return { ...locator, session };
}

function agent(agentExecutionId: string, executionStatus: string): AcpAgentExecutionVm {
  return {
    agentExecutionId,
    executionStatus,
    eventCount: 1,
    toolCallCount: 0,
    readFileCount: 0,
    writtenFileCount: 0,
    hasAttention: false,
    todoEntries: [],
  };
}

describe('conversation event router', () => {
  beforeEach(resetConversationEventRouterSnapshots);

  it('isolates snapshots by branch key', () => {
    applyConversationEventToBranchSnapshots(live('agent-a', uiEvent('toolCall', 'running')));
    expect(readConversationBranchLiveSnapshot(locator, 'agent-a')).toMatchObject({ status: 'running', revision: 1 });
    expect(readConversationBranchLiveSnapshot(locator, 'agent-b')).toMatchObject({ status: null, revision: 0 });
  });

  it('does not revise non-active tab state for every ordinary streaming event', () => {
    applyConversationEventToBranchSnapshots(live('agent-a', uiEvent('textDelta')));
    const first = readConversationBranchLiveSnapshot(locator, 'agent-a');
    applyConversationEventToBranchSnapshots(live('agent-a', { ...uiEvent('textDelta'), id: 'text-2', seq: 2 }));
    const second = readConversationBranchLiveSnapshot(locator, 'agent-a');
    expect(second).not.toBe(first);
    expect(second.revision).toBe(1);
    expect(second.contentRevision).toBe(2);
  });

  it('keeps an Agent queued when only its synthetic prompt has arrived', () => {
    applyConversationEventToBranchSnapshots(live('agent-a', {
      ...uiEvent('userTextDelta', 'completed'),
      raw: { source: 'agentBranchPrompt' },
    }));
    expect(readConversationBranchLiveSnapshot(locator, 'agent-a')).toMatchObject({
      status: null,
      revision: 0,
      contentRevision: 1,
    });

    applyConversationEventToBranchSnapshots(live('agent-a', { ...uiEvent('toolCall', 'running'), seq: 2 }));
    expect(readConversationBranchLiveSnapshot(locator, 'agent-a')).toMatchObject({ status: 'running', revision: 1 });
  });

  it('marks an Agent completed when its canonical branch result arrives', () => {
    applyConversationEventToBranchSnapshots(live('agent-a', uiEvent('toolCall', 'running')));
    applyConversationEventToBranchSnapshots(live('agent-a', {
      ...uiEvent('textDelta', 'completed'),
      seq: 2,
      raw: { source: 'agentBranchResult' },
    }));

    expect(readConversationBranchLiveSnapshot(locator, 'agent-a')).toMatchObject({
      status: 'completed',
      attention: false,
      revision: 2,
      contentRevision: 2,
    });
  });

  it('projects permission attention and clears it only after the decision event', () => {
    applyConversationEventToBranchSnapshots(live('agent-a', uiEvent('permissionRequest', 'pending')));
    expect(readConversationBranchLiveSnapshot(locator, 'agent-a')).toMatchObject({ status: 'waiting_permission', attention: true });
    applyConversationEventToBranchSnapshots(live('agent-a', uiEvent('permissionRequest', 'selected')));
    expect(readConversationBranchLiveSnapshot(locator, 'agent-a')).toMatchObject({ status: 'running', attention: false });
  });

  it('preserves completed branches and interrupts only still-active branches on root stop', () => {
    applyConversationEventToBranchSnapshots(live('agent-a', {
      ...uiEvent('textDelta', 'completed'),
      raw: { source: 'agentBranchResult' },
    }));
    applyConversationEventToBranchSnapshots(live('agent-b', uiEvent('toolCall', 'running')));

    applyConversationEventToBranchSnapshots(sessionUpdate('stopped', [agent('agent-a', 'completed')]));

    expect(readConversationBranchLiveSnapshot(locator, 'agent-a').status).toBe('completed');
    expect(readConversationBranchLiveSnapshot(locator, 'agent-b').status).toBe('interrupted');
  });

  it('accepts project-less live events for the matching attempt and rejects other attempts', () => {
    expect(conversationEventMatchesAttempt({ ...locator, projectId: null }, locator)).toBe(true);
    expect(conversationEventMatchesAttempt({ ...locator, attemptId: 'attempt-2' }, locator)).toBe(false);
  });
});
