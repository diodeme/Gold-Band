import { beforeEach, describe, expect, it } from 'vitest';

import type { AcpSessionUpdatedEventVm } from '@/api/client';
import {
  acknowledgeConversationBranchReplay,
  applyConversationEventToBranchSnapshots,
  CONVERSATION_EVENT_REPLAY_LIMITS,
  conversationEventMatchesAttempt,
  readConversationBranchLiveSnapshot,
  readConversationBranchReplaySnapshot,
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
    pendingElicitations: [],
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

  it('retains only the latest cumulative payload for the same live event', () => {
    applyConversationEventToBranchSnapshots(live('root', {
      ...uiEvent('textDelta'),
      id: 'answer-1',
      content: '检查',
      endedSeq: 2,
    }));
    applyConversationEventToBranchSnapshots(live('root', {
      ...uiEvent('textDelta'),
      id: 'answer-1',
      content: '检查完整内容',
      endedSeq: 3,
    }));

    const replay = readConversationBranchReplaySnapshot(locator, 'root');
    expect(replay).toMatchObject({ generation: 2, headSeq: 3, requiresCatchUp: false });
    expect(replay.events).toHaveLength(1);
    expect(replay.events[0]?.content).toBe('检查完整内容');
  });

  it('bounds retained events and marks the branch for afterSeq catch-up', () => {
    for (let index = 0; index <= CONVERSATION_EVENT_REPLAY_LIMITS.eventsPerBranch; index += 1) {
      applyConversationEventToBranchSnapshots(live('root', {
        ...uiEvent('toolCall'),
        id: `tool-${index}`,
        seq: index + 1,
        endedSeq: index + 1,
      }));
    }

    const replay = readConversationBranchReplaySnapshot(locator, 'root');
    expect(replay.events).toHaveLength(CONVERSATION_EVENT_REPLAY_LIMITS.eventsPerBranch);
    expect(replay.requiresCatchUp).toBe(true);
    expect(replay.headSeq).toBe(CONVERSATION_EVENT_REPLAY_LIMITS.eventsPerBranch + 1);
  });

  it('keeps only a watermark when one event exceeds the byte budget', () => {
    applyConversationEventToBranchSnapshots(live('root', {
      ...uiEvent('textDelta'),
      id: 'oversized-answer',
      endedSeq: 9,
      content: 'x'.repeat(CONVERSATION_EVENT_REPLAY_LIMITS.eventBytes),
    }));

    const replay = readConversationBranchReplaySnapshot(locator, 'root');
    expect(replay.events).toHaveLength(0);
    expect(replay).toMatchObject({ headSeq: 9, requiresCatchUp: true, retainedBytes: 0 });
  });

  it('evicts old branch payloads when the global byte budget is reached', () => {
    const branchIds = Array.from({ length: 20 }, (_, index) => `branch-${index}`);
    for (const [index, branchId] of branchIds.entries()) {
      applyConversationEventToBranchSnapshots(live(branchId, {
        ...uiEvent('textDelta'),
        id: `answer-${index}`,
        seq: index + 1,
        endedSeq: index + 1,
        content: 'x'.repeat(120_000),
      }));
    }

    const snapshots = branchIds.map((branchId) => (
      readConversationBranchReplaySnapshot(locator, branchId)
    ));
    expect(snapshots.reduce((total, snapshot) => total + snapshot.retainedBytes, 0))
      .toBeLessThanOrEqual(CONVERSATION_EVENT_REPLAY_LIMITS.globalBytes);
    expect(snapshots.some((snapshot) => snapshot.requiresCatchUp && snapshot.events.length === 0))
      .toBe(true);
  });

  it('acknowledges replay only when the snapshot covers the observed stable generation', () => {
    applyConversationEventToBranchSnapshots(live('root', {
      ...uiEvent('textDelta'),
      id: 'answer-1',
      endedSeq: 4,
      content: 'complete',
    }));
    const replay = readConversationBranchReplaySnapshot(locator, 'root');

    expect(acknowledgeConversationBranchReplay(locator, 'root', 3, replay.generation)).toBe(false);
    expect(acknowledgeConversationBranchReplay(locator, 'root', 4, replay.generation + 1)).toBe(false);
    expect(acknowledgeConversationBranchReplay(locator, 'root', 4, replay.generation)).toBe(true);
    expect(readConversationBranchReplaySnapshot(locator, 'root').events).toHaveLength(0);
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
