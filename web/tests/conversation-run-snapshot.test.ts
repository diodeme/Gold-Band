import { describe, expect, it } from 'vitest';
import {
  applyConversationBackgroundSessionRuntimeSnapshot,
  applyConversationSelectedSessionSnapshot,
  mergeConversationRunSnapshot,
} from '@/lib/conversation-run-snapshot';
import type { ConversationRunVm, ConversationSessionLeafVm, RuntimeDisplayVm } from '@/types';

const runningDisplay: RuntimeDisplayVm = {
  code: 'running',
  tone: 'running',
  icon: 'dot',
  terminal: false,
  resumable: false,
  reasonCode: null,
  blockingError: false,
};

const unknownDisplay: RuntimeDisplayVm = {
  code: 'unknown',
  tone: 'neutral',
  icon: 'dot',
  terminal: false,
  resumable: false,
  reasonCode: null,
  blockingError: false,
};

const pausedDisplay: RuntimeDisplayVm = {
  code: 'paused',
  tone: 'warning',
  icon: 'pause',
  terminal: false,
  resumable: true,
  reasonCode: 'process-interrupted',
  blockingError: false,
};

function leaf(
  status: string,
  runtimeDisplay: RuntimeDisplayVm,
  overrides: Partial<ConversationSessionLeafVm> = {},
): ConversationSessionLeafVm {
  const nodeId = overrides.nodeId ?? 'dev';
  const attemptId = overrides.attemptId ?? 'attempt-001';
  return {
    roundId: 'round-001',
    nodeId,
    attemptId,
    outerNodeId: null,
    outerAttemptId: null,
    pathLabel: `${nodeId}/${attemptId}`,
    status,
    outcome: null,
    runtimeDisplay,
    current: true,
    startedAt: '2026-06-12T00:00:00Z',
    finishedAt: null,
    sessionId: null,
    artifactCount: 0,
    attachmentCount: 0,
    ...overrides,
  };
}

function run(overrides: Partial<ConversationRunVm> = {}, attempts = [leaf('running', runningDisplay)]): ConversationRunVm {
  const selectedAttempt = attempts[0];
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
      selectedSessionKey: `${selectedAttempt.roundId}/${selectedAttempt.nodeId}/${selectedAttempt.attemptId}`,
      rounds: [{
        roundId: 'round-001',
        index: 1,
        label: 'round-001',
        status: 'running',
        runtimeDisplay: runningDisplay,
        nodes: attempts.map((attempt) => ({
          nodeId: attempt.nodeId,
          label: attempt.nodeId,
          nodeType: 'worker',
          status: attempt.status,
          runtimeDisplay: attempt.runtimeDisplay,
          attempts: [attempt],
          outerNodes: undefined,
        })),
      }],
    },
    selectedSession: { sessionId: 'session-1', status: 'running', events: [] } as any,
    activeSessions: [{
      roundId: selectedAttempt.roundId,
      nodeId: selectedAttempt.nodeId,
      attemptId: selectedAttempt.attemptId,
      outerNodeId: null,
      outerAttemptId: null,
      pathLabel: selectedAttempt.pathLabel,
      status: selectedAttempt.status,
      runtimeDisplay: selectedAttempt.runtimeDisplay,
      sessionId: null,
      startedAt: selectedAttempt.startedAt,
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

describe('applyConversationSelectedSessionSnapshot', () => {
  it('patches selected session when a full snapshot matches the selected identity', () => {
    const current = run({
      selectedSession: { sessionId: 'session-1', status: 'cancelling', events: [] } as any,
    });

    const patched = applyConversationSelectedSessionSnapshot(current, {
      taskId: 'task-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'dev',
      attemptId: 'attempt-001',
      session: { sessionId: 'session-1', status: 'cancelled', events: [{ content: 'stopped' }] } as any,
    });

    expect(patched?.selectedSession?.status).toBe('cancelled');
    expect(patched?.selectedSession?.events).toEqual([{ content: 'stopped' }]);
  });

  it('patches workflow graph status from a lifecycle-only selected snapshot', () => {
    const currentLeaf = leaf('paused', pausedDisplay, { current: true });
    const current = run({
      selectedSession: { sessionId: 'session-1', status: 'cancelled', events: [] } as any,
      workflowGraph: {
        nodes: [{
          id: '1:dev:attempt-001',
          nodeId: 'dev',
          sequence: 1,
          label: 'dev',
          nodeType: 'worker',
          status: 'paused',
          outcome: null,
          runtimeDisplay: pausedDisplay,
          attemptId: 'attempt-001',
          outerNodeId: null,
          outerAttemptId: null,
          attemptCount: 1,
          attempts: [{ attemptId: 'attempt-001', sequence: 1, status: 'paused', outcome: null, runtimeDisplay: pausedDisplay, current: true }],
          artifactCount: 0,
          attachmentCount: 0,
          current: true,
        }],
        edges: [],
      },
    }, [currentLeaf]);

    const patched = applyConversationSelectedSessionSnapshot(current, {
      taskId: 'task-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'dev',
      attemptId: 'attempt-001',
      lifecycle: {
        runtime: { status: 'running', outcome: null, pauseReason: null, resumable: false, current: true, active: true, continuable: false, phase: 'runtime-active' },
        acp: { status: 'cancelled', active: false, stopping: false, terminal: true },
        displayStatus: 'running',
        runtimeDisplay: runningDisplay,
        continueKind: null,
        composer: { mode: 'runtime-active', submitTarget: 'none', processingKind: 'processing', statusKey: null, canStop: true, lockInput: true, showContinueAction: false },
      },
    });

    expect(patched?.selectedSession?.status).toBe('cancelled');
    expect(patched?.sessionTree.rounds[0].nodes[0].attempts[0].status).toBe('running');
    expect(patched?.workflowGraph.nodes[0].status).toBe('running');
    expect(patched?.workflowGraph.nodes[0].runtimeDisplay.tone).toBe('running');
    expect(patched?.workflowGraph.nodes[0].attempts?.[0].status).toBe('running');
  });

  it('ignores full snapshots from non-selected session identities', () => {
    const devAttempt = leaf('completed', runningDisplay, { nodeId: 'dev', attemptId: 'attempt-001', current: false });
    const testAttempt = leaf('running', runningDisplay, { nodeId: 'test', attemptId: 'attempt-001', current: true });
    const current = run({
      selectedSession: { sessionId: 'dev-session', status: 'cancelled', events: [] } as any,
      sessionTree: {
        ...run({}, [devAttempt, testAttempt]).sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-001',
      },
    }, [devAttempt, testAttempt]);

    const patched = applyConversationSelectedSessionSnapshot(current, {
      taskId: 'task-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'test',
      attemptId: 'attempt-001',
      session: { sessionId: 'test-session', status: 'running', events: [{ content: 'test event' }] } as any,
    });

    expect(patched?.selectedSession?.sessionId).toBe('dev-session');
    expect(patched?.selectedSession?.status).toBe('cancelled');
  });
});

describe('applyConversationBackgroundSessionRuntimeSnapshot', () => {
  it('patches only background runtime identity fields and preserves the selected completed session payload', () => {
    const completedAttempt = leaf('completed', runningDisplay, { nodeId: 'dev', attemptId: 'attempt-001', current: false });
    const runningAttempt = leaf('running', runningDisplay, { nodeId: 'test', attemptId: 'attempt-001', current: true, sessionId: null });
    const current = run({
      selectedSession: { sessionId: 'dev-session', status: 'completed', events: [{ content: 'done' }] } as any,
      sessionTree: {
        ...run({}, [completedAttempt, runningAttempt]).sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-001',
      },
    }, [completedAttempt, runningAttempt]);

    const patched = applyConversationBackgroundSessionRuntimeSnapshot(current, {
      taskId: 'task-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'test',
      attemptId: 'attempt-001',
      session: {
        sessionId: 'test-session',
        status: 'running',
        sessionStartedAt: '2026-06-12T00:01:00Z',
        events: [],
      } as any,
    });

    expect(patched).not.toBe(current);
    expect(patched?.sessionTree.selectedSessionKey).toBe('round-001/dev/attempt-001');
    expect(patched?.selectedSession?.sessionId).toBe('dev-session');
    const testLeaf = patched?.sessionTree.rounds[0].nodes.find((node) => node.nodeId === 'test')?.attempts[0];
    expect(testLeaf?.sessionId).toBe('test-session');
    expect(patched?.activeSessions.find((session) => session.nodeId === 'test')?.sessionId).toBe('test-session');
  });

  it('returns the same run object when a repeated background snapshot does not change runtime identity', () => {
    const completedAttempt = leaf('completed', runningDisplay, { nodeId: 'dev', attemptId: 'attempt-001', current: false });
    const runningAttempt = leaf('running', runningDisplay, { nodeId: 'test', attemptId: 'attempt-001', current: true, sessionId: 'test-session' });
    const current = run({
      sessionTree: {
        ...run({}, [completedAttempt, runningAttempt]).sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-001',
      },
      activeSessions: [{
        roundId: 'round-001',
        nodeId: 'test',
        attemptId: 'attempt-001',
        outerNodeId: null,
        outerAttemptId: null,
        pathLabel: 'test/attempt-001',
        status: 'running',
        runtimeDisplay: runningDisplay,
        sessionId: 'test-session',
        startedAt: '2026-06-12T00:00:00Z',
      }],
    }, [completedAttempt, runningAttempt]);

    const patched = applyConversationBackgroundSessionRuntimeSnapshot(current, {
      taskId: 'task-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'test',
      attemptId: 'attempt-001',
      session: {
        sessionId: 'test-session',
        status: 'running',
        sessionStartedAt: '2026-06-12T00:00:00Z',
        events: [],
      } as any,
    });

    expect(patched).toBe(current);
  });

  it('does not patch the selected session through the background path', () => {
    const current = run();
    const patched = applyConversationBackgroundSessionRuntimeSnapshot(current, {
      taskId: 'task-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'dev',
      attemptId: 'attempt-001',
      session: { sessionId: 'session-2', status: 'running', events: [] } as any,
    });

    expect(patched).toBe(current);
  });

  it('does not revive a terminal background leaf from a stale running snapshot', () => {
    const selectedAttempt = leaf('completed', runningDisplay, { nodeId: 'dev', attemptId: 'attempt-001', current: false });
    const terminalAttempt = leaf('completed', runningDisplay, { nodeId: 'test', attemptId: 'attempt-001', current: false, sessionId: 'test-session' });
    const current = run({
      sessionTree: {
        ...run({}, [selectedAttempt, terminalAttempt]).sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-001',
      },
      activeSessions: [],
    }, [selectedAttempt, terminalAttempt]);

    const patched = applyConversationBackgroundSessionRuntimeSnapshot(current, {
      taskId: 'task-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'test',
      attemptId: 'attempt-001',
      session: {
        sessionId: 'test-session',
        status: 'running',
        sessionStartedAt: terminalAttempt.startedAt,
        events: [],
      } as any,
    });

    expect(patched).toBe(current);
  });
});

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

  it('preserves manual selection when live refresh returns a different selected key', () => {
    const testAttempt = leaf('completed', runningDisplay, { nodeId: 'test', attemptId: 'attempt-001', current: false });
    const devAttempt = leaf('running', runningDisplay, { nodeId: 'dev', attemptId: 'attempt-002', current: true });
    const current = run({
      sessionTree: {
        ...run({}, [testAttempt, devAttempt]).sessionTree,
        selectedSessionKey: 'round-001/test/attempt-001',
      },
    }, [testAttempt, devAttempt]);
    const incoming = run({
      sessionTree: {
        ...run({}, [testAttempt, devAttempt]).sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-002',
      },
    }, [testAttempt, devAttempt]);

    const merged = mergeConversationRunSnapshot(current, incoming, 'live-refresh', {
      preserveSelectedSession: true,
    });

    expect(merged.sessionTree.selectedSessionKey).toBe('round-001/test/attempt-001');
  });

  it('keeps selected session payload when preserving manual selection', () => {
    const testAttempt = leaf('completed', runningDisplay, { nodeId: 'test', attemptId: 'attempt-001', current: false });
    const devAttempt = leaf('running', runningDisplay, { nodeId: 'dev', attemptId: 'attempt-002', current: true });
    const current = run({
      selectedSession: { sessionId: 'test-session', status: 'completed', events: [] } as any,
      sessionTree: {
        ...run({}, [testAttempt, devAttempt]).sessionTree,
        selectedSessionKey: 'round-001/test/attempt-001',
      },
    }, [testAttempt, devAttempt]);
    const incoming = run({
      selectedSession: { sessionId: 'dev-session', status: 'running', events: [] } as any,
      sessionTree: {
        ...run({}, [testAttempt, devAttempt]).sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-002',
      },
    }, [testAttempt, devAttempt]);

    const merged = mergeConversationRunSnapshot(current, incoming, 'live-refresh', {
      preserveSelectedSession: true,
    });

    expect(merged.sessionTree.selectedSessionKey).toBe('round-001/test/attempt-001');
    expect(merged.selectedSession?.sessionId).toBe('test-session');
  });

  it('uses the preferred selected key when auto-follow requests it', () => {
    const testAttempt = leaf('completed', runningDisplay, { nodeId: 'test', attemptId: 'attempt-001', current: false });
    const devAttempt = leaf('running', runningDisplay, { nodeId: 'dev', attemptId: 'attempt-002', current: true });
    const current = run({
      sessionTree: {
        ...run({}, [testAttempt, devAttempt]).sessionTree,
        selectedSessionKey: 'round-001/test/attempt-001',
      },
    }, [testAttempt, devAttempt]);
    const incoming = run({
      selectedSession: { sessionId: 'test-session', status: 'completed', events: [{ content: 'test event' }] } as any,
      sessionTree: {
        ...run({}, [testAttempt, devAttempt]).sessionTree,
        selectedSessionKey: 'round-001/test/attempt-001',
      },
    }, [testAttempt, devAttempt]);

    const merged = mergeConversationRunSnapshot(current, incoming, 'live-refresh', {
      selectedSessionKey: 'round-001/dev/attempt-002',
    });

    expect(merged.sessionTree.selectedSessionKey).toBe('round-001/dev/attempt-002');
    expect(merged.selectedSession).toBeNull();
    expect(merged.artifacts).toEqual([]);
    expect(merged.attachments).toEqual([]);
  });

  it('preserves selected session payload when same-key refresh omits it', () => {
    const current = run({
      runStatus: 'paused',
      selectedSession: { sessionId: 'session-1', status: 'cancelled', events: [{ content: 'stopped' }] } as any,
    });
    const incoming = run({
      runStatus: 'paused',
      selectedSession: null,
      activeSessions: [],
      sessionTree: {
        ...current.sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-001',
      },
    });

    const merged = mergeConversationRunSnapshot(current, incoming, 'live-refresh');

    expect(merged.sessionTree.selectedSessionKey).toBe('round-001/dev/attempt-001');
    expect(merged.selectedSession?.status).toBe('cancelled');
    expect(merged.selectedSession?.sessionId).toBe('session-1');
  });

  it('replaces selected session payload when a same-key full snapshot arrives', () => {
    const current = run({
      runStatus: 'paused',
      selectedSession: { sessionId: 'session-1', status: 'cancelling', events: [{ content: 'stopping' }] } as any,
    });
    const incoming = run({
      runStatus: 'paused',
      selectedSession: { sessionId: 'session-1', status: 'cancelled', events: [{ content: 'stopped' }] } as any,
      activeSessions: [],
      sessionTree: {
        ...current.sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-001',
      },
    });

    const merged = mergeConversationRunSnapshot(current, incoming, 'live-refresh');

    expect(merged.sessionTree.selectedSessionKey).toBe('round-001/dev/attempt-001');
    expect(merged.selectedSession?.status).toBe('cancelled');
    expect(merged.selectedSession?.events).toEqual([{ content: 'stopped' }]);
  });

  it('does not preserve selected session payload after the selected identity changes', () => {
    const devAttempt = leaf('completed', runningDisplay, { nodeId: 'dev', attemptId: 'attempt-001', current: false });
    const testAttempt = leaf('running', runningDisplay, { nodeId: 'test', attemptId: 'attempt-001', current: true });
    const current = run({
      selectedSession: { sessionId: 'dev-session', status: 'cancelled', events: [{ content: 'stopped' }] } as any,
      sessionTree: {
        ...run({}, [devAttempt, testAttempt]).sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-001',
      },
    }, [devAttempt, testAttempt]);
    const incoming = run({
      selectedSession: null,
      sessionTree: {
        ...run({}, [devAttempt, testAttempt]).sessionTree,
        selectedSessionKey: 'round-001/test/attempt-001',
      },
    }, [devAttempt, testAttempt]);

    const merged = mergeConversationRunSnapshot(current, incoming, 'live-refresh');

    expect(merged.sessionTree.selectedSessionKey).toBe('round-001/test/attempt-001');
    expect(merged.selectedSession).toBeNull();
  });

  it('does not attach stale selected session payload to a newly selected session', () => {
    const acceptAttempt = leaf('running', runningDisplay, { nodeId: 'accept', attemptId: 'attempt-001', current: true });
    const devAttempt = leaf('completed', runningDisplay, { nodeId: 'dev', attemptId: 'attempt-002', current: false });
    const current = run({
      selectedSession: null,
      sessionTree: {
        ...run({}, [acceptAttempt, devAttempt]).sessionTree,
        selectedSessionKey: 'round-001/accept/attempt-001',
      },
    }, [acceptAttempt, devAttempt]);
    const incoming = run({
      selectedSession: { sessionId: 'dev-session', status: 'completed', events: [{ content: 'dev event' }] } as any,
      artifacts: [{ name: 'dev-report.md' }] as any,
      attachments: [{ name: 'dev-report.md' }] as any,
      sessionTree: {
        ...run({}, [acceptAttempt, devAttempt]).sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-002',
      },
    }, [acceptAttempt, devAttempt]);

    const merged = mergeConversationRunSnapshot(current, incoming, 'live-refresh', {
      selectedSessionKey: 'round-001/accept/attempt-001',
    });

    expect(merged.sessionTree.selectedSessionKey).toBe('round-001/accept/attempt-001');
    expect(merged.selectedSession).toBeNull();
    expect(merged.artifacts).toEqual([]);
    expect(merged.attachments).toEqual([]);
  });

  it('replaces state when the snapshot belongs to a different run', () => {
    const current = run();
    const incoming = run({ runId: 'run-002', sessionTree: { rounds: [], selectedSessionKey: null } });

    const merged = mergeConversationRunSnapshot(current, incoming, 'rerun');

    expect(merged.runId).toBe('run-002');
    expect(merged.sessionTree.rounds).toHaveLength(0);
  });
});
