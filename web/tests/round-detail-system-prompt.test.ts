import { describe, expect, it } from 'vitest';
import { acpOptimisticKey, buildConversationSystemPromptOptions, mergedConversationSession } from '../src/pages/RoundDetailPage';
import type { AcpConversationVm, AcpSessionVm } from '../src/types';

function makeSession(partial?: Partial<AcpSessionVm>): AcpSessionVm {
  return {
    provider: 'claude-acp',
    status: 'running',
    restored: false,
    systemPromptAppend: null,
    events: [],
    timelineProjection: null,
    eventPage: {
      total: 0,
      loadedCount: 0,
      oldestSeq: null,
      newestSeq: null,
      hasOlder: false,
      hasNewer: false,
      oldestCursor: null,
      newestCursor: null,
    },
    pendingInteractions: [],
    diagnostics: {
      rawFrameCount: 0,
      eventCount: 0,
      errorCount: 0,
      lastError: null,
      lastErrorTimestamp: null,
    },
    ...partial,
  };
}

function makeConversation(): AcpConversationVm {
  return {
    key: 'session:s-1',
    label: 'Session 1',
    sessionId: 's-1',
    sessionMode: 'new',
    activeAttemptId: 'attempt-001',
    attempts: [
      {
        nodeId: 'node-1',
        attemptId: 'attempt-001',
        status: 'running',
        current: true,
        acpSessionId: 's-1',
        acpSession: makeSession(),
      },
    ],
  };
}

describe('round detail system prompt fallback', () => {
  it('isolates optimistic events by complete session and branch identity', () => {
    const root = acpOptimisticKey('project-1', 'task-1', 'run-1', 'round-1', 'node-1', 'attempt-1', null, null, 'root');
    const branch = acpOptimisticKey('project-1', 'task-1', 'run-1', 'round-1', 'node-1', 'attempt-1', null, null, 'agent-1');
    const dynamic = acpOptimisticKey('project-1', 'task-1', 'run-1', 'round-1', 'node-1', 'attempt-1', 'outer-1', 'outer-attempt-1', 'root');

    expect(new Set([root, branch, dynamic]).size).toBe(3);
  });

  it('keeps the current attempt system prompt before conversation snapshot catches up', () => {
    const conversation = makeConversation();
    const fallback = makeSession({ systemPromptAppend: 'current system prompt' });

    const merged = mergedConversationSession(conversation, fallback);

    expect(merged?.systemPromptAppend).toBe('current system prompt');
  });

  it('fills the missing attempt option from the current fallback session', () => {
    const conversation = makeConversation();
    const fallback = makeSession({ systemPromptAppend: 'current system prompt' });

    expect(
      buildConversationSystemPromptOptions(
        conversation,
        fallback,
        'attempt-001',
      ),
    ).toEqual([
      { attemptId: 'attempt-001', prompt: 'current system prompt' },
    ]);
  });

  it('inserts explicit stopped and continued boundaries between attempts', () => {
    const first = makeSession({
      status: 'cancelled',
      stopReason: 'cancelled',
      events: [{
        id: 'first-text', seq: 1, timestamp: '1Z', kind: 'textDelta',
        sessionId: 's-1', content: 'first', title: null, toolCallId: null,
        status: null, raw: null,
      }],
    });
    const second = makeSession({
      status: 'running',
      events: [{
        id: 'second-text', seq: 1, timestamp: '2Z', kind: 'textDelta',
        sessionId: 's-1', content: 'second', title: null, toolCallId: null,
        status: null, raw: null,
      }],
    });
    const conversation: AcpConversationVm = {
      ...makeConversation(),
      activeAttemptId: 'attempt-002',
      attempts: [
        { nodeId: 'node-1', attemptId: 'attempt-001', status: 'paused', current: false, acpSessionId: 's-1', acpSession: first },
        { nodeId: 'node-1', attemptId: 'attempt-002', status: 'running', current: true, acpSessionId: 's-1', acpSession: second },
      ],
    };

    const merged = mergedConversationSession(conversation);
    const boundaries = merged?.events
      .filter((event) => event.kind === 'attemptSeparator')
      .map((event) => (event.raw as { boundaryKind?: string })?.boundaryKind);
    expect(boundaries).toEqual(['stopped', 'continued']);
  });

  it('keeps pagination owned by the active attempt semantic page', () => {
    const first = makeSession({
      events: Array.from({ length: 50 }, (_, index) => ({
        id: `tool-${index}`,
        seq: index + 1,
        timestamp: `${index + 1}Z`,
        kind: 'toolCall',
        sessionId: 's-1',
        content: null,
        title: 'Read',
        toolCallId: `call-${index}`,
        status: 'completed',
        raw: null,
      })),
    });
    const activePage = {
      total: 2,
      loadedCount: 2,
      oldestSeq: 10,
      newestSeq: 20,
      hasOlder: false,
      hasNewer: false,
      oldestCursor: 'seq:10',
      newestCursor: 'seq:20',
    };
    const second = makeSession({ eventPage: activePage });
    const conversation: AcpConversationVm = {
      ...makeConversation(),
      activeAttemptId: 'attempt-002',
      attempts: [
        { nodeId: 'node-1', attemptId: 'attempt-001', status: 'completed', current: false, acpSessionId: 's-1', acpSession: first },
        { nodeId: 'node-1', attemptId: 'attempt-002', status: 'running', current: true, acpSessionId: 's-1', acpSession: second },
      ],
    };

    expect(mergedConversationSession(conversation)?.eventPage).toEqual(activePage);
  });
});
