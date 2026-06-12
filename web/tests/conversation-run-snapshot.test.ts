import { describe, expect, it } from 'vitest';
import { mergeConversationRunSnapshot } from '@/lib/conversation-run-snapshot';
import type { ConversationRunVm, ConversationSessionLeafVm, RuntimeDisplayVm } from '@/types';

const runningDisplay: RuntimeDisplayVm = {
  code: 'running',
  tone: 'running',
  icon: 'dot',
  terminal: false,
  resumable: false,
  reasonCode: null,
};

const unknownDisplay: RuntimeDisplayVm = {
  code: 'unknown',
  tone: 'neutral',
  icon: 'dot',
  terminal: false,
  resumable: false,
  reasonCode: null,
};

function leaf(status: string, runtimeDisplay: RuntimeDisplayVm): ConversationSessionLeafVm {
  return {
    roundId: 'round-001',
    nodeId: 'dev',
    attemptId: 'attempt-001',
    outerNodeId: null,
    outerAttemptId: null,
    pathLabel: 'dev/attempt-001',
    status,
    outcome: null,
    runtimeDisplay,
    current: true,
    startedAt: '2026-06-12T00:00:00Z',
    finishedAt: null,
    sessionId: null,
    artifactCount: 0,
    attachmentCount: 0,
  };
}

function run(overrides: Partial<ConversationRunVm> = {}): ConversationRunVm {
  const attempt = leaf('running', runningDisplay);
  return {
    projectId: 'default',
    taskId: 'task-001',
    runId: 'run-001',
    title: 'Task',
    autoTitle: true,
    runMode: 'workflow',
    workflowTemplateId: null,
    runStatus: 'running',
    runOutcome: null,
    sessionTree: {
      selectedSessionKey: 'round-001/dev/attempt-001',
      rounds: [{
        roundId: 'round-001',
        index: 1,
        label: 'round-001',
        status: 'running',
        runtimeDisplay: runningDisplay,
        nodes: [{
          nodeId: 'dev',
          label: 'dev',
          nodeType: 'worker',
          status: attempt.status,
          runtimeDisplay: attempt.runtimeDisplay,
          attempts: [attempt],
          outerNodes: undefined,
        }],
      }],
    },
    selectedSession: { sessionId: 'session-1', status: 'running', events: [] } as any,
    activeSessions: [{
      roundId: attempt.roundId,
      nodeId: attempt.nodeId,
      attemptId: attempt.attemptId,
      outerNodeId: null,
      outerAttemptId: null,
      pathLabel: attempt.pathLabel,
      status: attempt.status,
      runtimeDisplay: attempt.runtimeDisplay,
      sessionId: null,
      startedAt: attempt.startedAt,
    }],
    artifacts: [],
    attachments: [],
    inputAttachments: [],
    workflowStatus: 'valid',
    workflowValid: true,
    workflowError: null,
    workflowJson: null,
    workflowGraph: { nodes: [], edges: [] },
    resumable: false,
    pauseReason: null,
    ...overrides,
  };
}

function withLeaf(base: ConversationRunVm, nextLeaf: ConversationSessionLeafVm): ConversationRunVm {
  return {
    ...base,
    sessionTree: {
      ...base.sessionTree,
      rounds: base.sessionTree.rounds.map((round) => ({
        ...round,
        nodes: round.nodes.map((node) => ({
          ...node,
          status: nextLeaf.status,
          runtimeDisplay: nextLeaf.runtimeDisplay,
          attempts: [nextLeaf],
        })),
      })),
    },
  };
}

describe('mergeConversationRunSnapshot', () => {
  it('does not let an initial unknown ACP snapshot downgrade a running runtime leaf', () => {
    const current = run();
    const incoming = {
      ...withLeaf(current, leaf('unknown', unknownDisplay)),
      selectedSession: { sessionId: null, status: 'unknown', events: [] } as any,
      activeSessions: [],
    };

    const merged = mergeConversationRunSnapshot(current, incoming, 'initial-load');
    const mergedLeaf = merged.sessionTree.rounds[0].nodes[0].attempts[0];

    expect(mergedLeaf.status).toBe('running');
    expect(mergedLeaf.runtimeDisplay.tone).toBe('running');
    expect(merged.activeSessions).toHaveLength(1);
    expect(merged.selectedSession?.status).toBe('running');
  });

  it('preserves the current selected key when an incoming same-run snapshot omits it', () => {
    const current = run();
    const incoming = {
      ...current,
      sessionTree: { ...current.sessionTree, selectedSessionKey: null },
    };

    const merged = mergeConversationRunSnapshot(current, incoming, 'live-refresh');

    expect(merged.sessionTree.selectedSessionKey).toBe('round-001/dev/attempt-001');
  });

  it('replaces state when the snapshot belongs to a different run', () => {
    const current = run();
    const incoming = run({ runId: 'run-002', sessionTree: { rounds: [], selectedSessionKey: null } });

    const merged = mergeConversationRunSnapshot(current, incoming, 'rerun');

    expect(merged.runId).toBe('run-002');
    expect(merged.sessionTree.rounds).toHaveLength(0);
  });
});
