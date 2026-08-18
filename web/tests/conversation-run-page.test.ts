import { describe, expect, it } from 'vitest';
import {
  resolveConversationContentQueryState,
  sessionBelongsToLeaf,
  shouldShowConversationContentLoadingState,
} from '../src/pages/ConversationRunPage';
import type { AcpSessionVm, ConversationRunVm, ConversationSessionLeafVm } from '../src/types';

function run(partial: Partial<ConversationRunVm> = {}): ConversationRunVm {
  return {
    projectId: 'default',
    taskId: 'task-081',
    runId: 'run-001',
    runStatus: 'running',
    runMode: 'workflow',
    sessionTree: { rounds: [], selectedSessionKey: null },
    selectedSession: null,
    activeSessions: [],
    inputAttachments: [],
    workflowValid: true,
    workflowStatus: 'valid',
    workflowGraph: null,
    ...partial,
  } as ConversationRunVm;
}

function leaf(partial: Partial<ConversationSessionLeafVm> = {}): ConversationSessionLeafVm {
  return {
    roundId: 'round-001',
    nodeId: 'goodbye-output',
    attemptId: 'attempt-001',
    outerNodeId: 'ai-dynamic',
    outerAttemptId: 'attempt-001',
    pathLabel: 'round-001/ai-dynamic/attempt-001/goodbye-output/attempt-001',
    status: 'running',
    current: true,
    manualCheckPending: false,
    artifactCount: 0,
    attachmentCount: 0,
    ...partial,
  } as ConversationSessionLeafVm;
}

function session(partial: Partial<AcpSessionVm> = {}): AcpSessionVm {
  return {
    sessionId: 'session-1',
    provider: 'codex-acp',
    status: 'running',
    restored: false,
    events: [],
    eventPage: {
      loadedCount: 0,
      total: 0,
      hasOlder: false,
      hasNewer: false,
    },
    pendingPermissions: [],
    pendingElicitations: [],
    diagnostics: {
      rawFrameCount: 0,
      eventCount: 0,
      errorCount: 0,
    },
    ...partial,
  };
}

describe('ConversationRunPage session leaf matching', () => {
  it('uses session identity fields before cwd for dynamic continue sessions', () => {
    const selectedLeaf = leaf();
    const selectedSession = session({
      roundId: 'round-001',
      nodeId: 'goodbye-output',
      attemptId: 'attempt-001',
      outerNodeId: 'ai-dynamic',
      outerAttemptId: 'attempt-001',
      cwd: 'D:\\Projects\\code\\ai\\Gold-Band',
      providerCwd: 'D:\\Projects\\code\\ai\\Gold-Band',
    });

    expect(sessionBelongsToLeaf(selectedSession, run(), selectedLeaf)).toBe(true);
  });

  it('rejects identity mismatches even when cwd is generic', () => {
    const selectedLeaf = leaf();
    const selectedSession = session({
      roundId: 'round-001',
      nodeId: 'hello-output',
      attemptId: 'attempt-001',
      outerNodeId: 'ai-dynamic',
      outerAttemptId: 'attempt-001',
      cwd: 'D:\\Projects\\code\\ai\\Gold-Band',
    });

    expect(sessionBelongsToLeaf(selectedSession, run(), selectedLeaf)).toBe(false);
  });
});

describe('ConversationRunPage content loading gate', () => {
  it('shows hydrated cached content immediately while it revalidates', () => {
    expect(resolveConversationContentQueryState('session-a', null, true)).toBe('success');
  });

  it('loads an uncached identity and ignores a previous session projection', () => {
    expect(resolveConversationContentQueryState(
      'session-b',
      { identity: 'session-a', state: 'success' },
      false,
    )).toBe('loading');
  });

  it('keeps a current session on its runtime-projected chat shell before establishment', () => {
    expect(shouldShowConversationContentLoadingState('loading', {
      current: true,
      sessionEstablished: false,
    })).toBe(false);
  });

  it('keeps a current session on the same chat shell after establishment', () => {
    expect(shouldShowConversationContentLoadingState('loading', {
      current: true,
      sessionEstablished: true,
    })).toBe(false);
  });

  it('keeps the isolated content loading state for an uncached historical session', () => {
    expect(shouldShowConversationContentLoadingState('loading', {
      current: false,
      sessionEstablished: true,
    })).toBe(true);
  });

  it('does not show the content loading state after hydration completes', () => {
    expect(shouldShowConversationContentLoadingState('success', {
      current: true,
      sessionEstablished: false,
    })).toBe(false);
  });
});
