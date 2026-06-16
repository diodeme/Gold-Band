import { describe, expect, it } from 'vitest';
import { buildConversationSystemPromptOptions, mergedConversationSession } from '../src/pages/RoundDetailPage';
import type { AcpConversationVm, AcpSessionVm } from '../src/types';

function makeSession(partial?: Partial<AcpSessionVm>): AcpSessionVm {
  return {
    provider: 'claude-acp',
    status: 'running',
    restored: false,
    systemPromptAppend: null,
    events: [],
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
    pendingPermissions: [],
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
});
