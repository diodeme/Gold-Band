import { describe, expect, it } from 'vitest';
import { ConversationRunCache } from '@/lib/conversation-run-cache';
import {
  applyConversationBackgroundSessionRuntimeSnapshot,
  applyConversationSelectedSessionSnapshot,
  conversationSessionKeyFromParts,
  isConversationActiveStatus,
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

const runtimeAbnormalDisplay: RuntimeDisplayVm = {
  code: 'runtime-abnormal',
  tone: 'danger',
  icon: 'error',
  terminal: false,
  resumable: true,
  reasonCode: 'runtime-abnormal',
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
    manualCheckPending: false,
    startedAt: '2026-06-12T00:00:00Z',
    finishedAt: null,
    sessionId: null,
    workLocation: { kind: 'main' },
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
    taskUuid: 'task-uuid-001',
    runId: 'run-001',
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
    activeSessions: [{
      roundId: selectedAttempt.roundId,
      nodeId: selectedAttempt.nodeId,
      attemptId: selectedAttempt.attemptId,
      outerNodeId: null,
      outerAttemptId: null,
      pathLabel: selectedAttempt.pathLabel,
      status: selectedAttempt.status,
      runtimeDisplay: selectedAttempt.runtimeDisplay,
      manualCheckPending: selectedAttempt.manualCheckPending,
      sessionId: null,
      startedAt: selectedAttempt.startedAt,
    }],
    inputAttachments: [],
    workflowStatus: 'valid',
    workflowValid: true,
    workflowError: null,
    workflowJson: null,
    workflowGraph: { nodes: [], edges: [], groups: [] },
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

describe('conversationSessionKeyFromParts', () => {
  it('builds the selected session key from run-state update identity', () => {
    expect(conversationSessionKeyFromParts({
      roundId: 'round-001',
      nodeId: 'plan',
      attemptId: 'attempt-001',
    })).toBe('round-001/plan/attempt-001');
  });
});

describe('isConversationActiveStatus', () => {
  it('treats dynamic ready and normalized active statuses as active', () => {
    expect(isConversationActiveStatus('ready')).toBe(true);
    expect(isConversationActiveStatus('in_progress')).toBe(true);
    expect(isConversationActiveStatus('cancel_requested')).toBe(true);
    expect(isConversationActiveStatus('completed')).toBe(false);
  });

  it('does not treat runtime-abnormal as an active running status', () => {
    expect(isConversationActiveStatus('runtime-abnormal')).toBe(false);
  });
});

describe('runtime summary locator isolation', () => {
  it.each([
    ['selected', applyConversationSelectedSessionSnapshot],
    ['background', applyConversationBackgroundSessionRuntimeSnapshot],
  ] as const)('rejects foreign or incomplete locator on the %s path', (path, applySnapshot) => {
    const attempts = [leaf('running', runningDisplay), leaf('running', runningDisplay, { nodeId: 'test' })];
    const current = run({}, attempts);
    const snapshot = {
      projectId: current.projectId,
      taskId: current.taskId,
      taskUuid: current.taskUuid!,
      runId: current.runId,
      roundId: 'round-001',
      nodeId: path === 'selected' ? 'dev' : 'test',
      attemptId: 'attempt-001',
      sessionId: 'confirmed-session',
      sessionStartedAt: '2026-10-14T07:00:00Z',
    };

    for (const mismatch of [
      { projectId: 'other-project' },
      { projectId: '' },
      { taskUuid: 'recreated-task' },
      { taskUuid: '' },
      { runId: 'other-run' },
      { roundId: 'other-round' },
    ]) {
      expect(applySnapshot(current, { ...snapshot, ...mismatch })).toBe(current);
    }
    const patched = applySnapshot(current, snapshot);
    expect(patched).not.toBe(current);
    expect(patched).not.toHaveProperty('selectedSession');
    expect(patched?.sessionTree.rounds[0].nodes.find((node) => node.nodeId === snapshot.nodeId)?.attempts[0])
      .toMatchObject({ sessionId: snapshot.sessionId, startedAt: snapshot.sessionStartedAt });
    expect(applySnapshot(patched, snapshot)).toBe(patched);
  });
});

describe('runtime-abnormal snapshots', () => {
  it('removes paused abnormal sessions from active sessions while preserving selected leaf state', () => {
    const current = run();
    const abnormalLeaf = leaf('paused', runtimeAbnormalDisplay, { current: true });

    const patched = applyConversationSelectedSessionSnapshot(current, {
      projectId: 'default',
      taskId: 'task-001',
      taskUuid: 'task-uuid-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'dev',
      attemptId: 'attempt-001',
      lifecycle: {
        runtime: { status: 'paused', outcome: null, pauseReason: 'runtime-abnormal', resumable: true, current: true, active: false, continuable: true, phase: 'paused' },
        control: { mode: 'non-runtime-controlled' },
        acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'cancelled', stopping: false },
        displayStatus: 'paused',
        runtimeDisplay: runtimeAbnormalDisplay,
        continueKind: 'continue-current-attempt',
        composer: { mode: 'normal', submitTarget: 'acp-prompt', processingKind: 'processing', statusKey: null, canStop: false, lockInput: false },
      },
    });

    expect(patched?.activeSessions).toEqual([]);
    expect(patched?.sessionTree.rounds[0].nodes[0].attempts[0].runtimeDisplay).toEqual(abnormalLeaf.runtimeDisplay);
  });
});

describe('applyConversationSelectedSessionSnapshot', () => {
  it('accepts a lightweight session identity snapshot with the complete locator', () => {
    const current = run();
    const patched = applyConversationSelectedSessionSnapshot(current, {
      projectId: 'default',
      taskId: 'task-001',
      taskUuid: 'task-uuid-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'dev',
      attemptId: 'attempt-001',
      sessionId: 'canonical-session',
      sessionStartedAt: '2026-10-14T07:00:00Z',
    });

    expect(patched?.sessionTree.rounds[0].nodes[0].attempts[0].sessionId).toBe('canonical-session');
    expect(patched?.sessionTree.rounds[0].nodes[0].attempts[0].startedAt).toBe('2026-10-14T07:00:00Z');
  });

  it('patches selected session identity without retaining ACP content', () => {
    const current = run({
    });

    const patched = applyConversationSelectedSessionSnapshot(current, {
      projectId: 'default',
      taskId: 'task-001',
      taskUuid: 'task-uuid-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'dev',
      attemptId: 'attempt-001',
      sessionId: 'session-1',
      sessionStartedAt: '2026-06-12T00:01:00Z',
    });

    expect(patched?.sessionTree.rounds[0].nodes[0].attempts[0].sessionId).toBe('session-1');
    expect(patched?.sessionTree.rounds[0].nodes[0].attempts[0].startedAt).toBe('2026-06-12T00:01:00Z');
    expect(patched).not.toHaveProperty('selectedSession');
  });

  it('patches workflow graph status from a lifecycle-only selected snapshot', () => {
    const currentLeaf = leaf('paused', pausedDisplay, { current: true });
    const current = run({
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
        groups: [],
      },
    }, [currentLeaf]);

    const patched = applyConversationSelectedSessionSnapshot(current, {
      projectId: 'default',
      taskId: 'task-001',
      taskUuid: 'task-uuid-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'dev',
      attemptId: 'attempt-001',
      lifecycle: {
        runtime: { status: 'running', outcome: null, pauseReason: null, resumable: false, current: true, active: true, continuable: false, phase: 'runtime-active' },
        control: { mode: 'runtime-controlled' },
        acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'cancelled', stopping: false },
        displayStatus: 'running',
        runtimeDisplay: runningDisplay,
        continueKind: null,
        composer: { mode: 'runtime-active', submitTarget: 'none', processingKind: 'processing', statusKey: null, canStop: true, lockInput: true },
      },
    });

    expect(patched).not.toHaveProperty('selectedSession');
    expect(patched?.sessionTree.rounds[0].nodes[0].attempts[0].status).toBe('running');
    expect(patched?.workflowGraph.nodes[0].status).toBe('running');
    expect(patched?.workflowGraph.nodes[0].runtimeDisplay.tone).toBe('running');
    expect(patched?.workflowGraph.nodes[0].attempts?.[0].status).toBe('running');
  });

  it('ignores selected-path summaries from non-selected session identities', () => {
    const devAttempt = leaf('completed', runningDisplay, { nodeId: 'dev', attemptId: 'attempt-001', current: false });
    const testAttempt = leaf('running', runningDisplay, { nodeId: 'test', attemptId: 'attempt-001', current: true });
    const current = run({
      sessionTree: {
        ...run({}, [devAttempt, testAttempt]).sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-001',
      },
    }, [devAttempt, testAttempt]);

    const patched = applyConversationSelectedSessionSnapshot(current, {
      projectId: 'default',
      taskId: 'task-001',
      taskUuid: 'task-uuid-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'test',
      attemptId: 'attempt-001',
      sessionId: 'test-session',
    });

    expect(patched?.sessionTree.rounds[0].nodes[0].attempts[0]).toBe(current.sessionTree.rounds[0].nodes[0].attempts[0]);
    expect(patched).not.toHaveProperty('selectedSession');
  });
});

describe('applyConversationBackgroundSessionRuntimeSnapshot', () => {
  it('patches only background runtime identity fields and preserves the selected completed leaf', () => {
    const completedAttempt = leaf('completed', runningDisplay, { nodeId: 'dev', attemptId: 'attempt-001', current: false });
    const runningAttempt = leaf('running', runningDisplay, { nodeId: 'test', attemptId: 'attempt-001', current: true, sessionId: null });
    const current = run({
      sessionTree: {
        ...run({}, [completedAttempt, runningAttempt]).sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-001',
      },
    }, [completedAttempt, runningAttempt]);

    const patched = applyConversationBackgroundSessionRuntimeSnapshot(current, {
      projectId: 'default',
      taskId: 'task-001',
      taskUuid: 'task-uuid-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'test',
      attemptId: 'attempt-001',
      sessionId: 'test-session',
      sessionStartedAt: '2026-06-12T00:01:00Z',
    });

    expect(patched).not.toBe(current);
    expect(patched?.sessionTree.selectedSessionKey).toBe('round-001/dev/attempt-001');
    expect(patched?.sessionTree.rounds[0].nodes[0].attempts[0]).toBe(current.sessionTree.rounds[0].nodes[0].attempts[0]);
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
        manualCheckPending: false,
        sessionId: 'test-session',
        startedAt: '2026-06-12T00:00:00Z',
      }],
    }, [completedAttempt, runningAttempt]);

    const patched = applyConversationBackgroundSessionRuntimeSnapshot(current, {
      projectId: 'default',
      taskId: 'task-001',
      taskUuid: 'task-uuid-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'test',
      attemptId: 'attempt-001',
      sessionId: 'test-session',
      sessionStartedAt: '2026-06-12T00:00:00Z',
    });

    expect(patched).toBe(current);
  });

  it('does not patch the selected session through the background path', () => {
    const current = run();
    const patched = applyConversationBackgroundSessionRuntimeSnapshot(current, {
      projectId: 'default',
      taskId: 'task-001',
      taskUuid: 'task-uuid-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'dev',
      attemptId: 'attempt-001',
      sessionId: 'session-2',
    });

    expect(patched).toBe(current);
  });

  it('does not revive a terminal background leaf from an identity-only summary', () => {
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
      projectId: 'default',
      taskId: 'task-001',
      taskUuid: 'task-uuid-001',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'test',
      attemptId: 'attempt-001',
      sessionId: 'test-session',
      sessionStartedAt: terminalAttempt.startedAt,
    });

    expect(patched).toBe(current);
  });
});

describe('mergeConversationRunSnapshot', () => {
  it('does not retain duplicate ACP content when a lightweight run refresh is cached', () => {
    const current = Object.assign(run(), {
      selectedSession: { sessionId: 'session-1', pendingElicitations: [{ interactionId: 'settled-question' }] },
    });
    const incoming = run();

    const merged = mergeConversationRunSnapshot(current, incoming, 'live-refresh');
    const cache = new ConversationRunCache();
    cache.store(merged);

    expect(merged).not.toHaveProperty('selectedSession');
    expect(cache.restore(incoming)).not.toHaveProperty('selectedSession');
  });

  it('does not carry a deleted task session into a new task that reuses the same sequential locator', () => {
    const oldAttempt = leaf('completed', runningDisplay, {
      nodeId: 'direct-agent',
      sessionId: 'old-session',
      sessionEstablished: true,
    });
    const current = run({
      projectId: 'project-1',
      taskId: 'task-004',
      taskUuid: 'old-task-uuid',
    }, [oldAttempt]);
    const newAttempt = leaf('running', runningDisplay, {
      nodeId: 'direct-agent',
      sessionId: null,
      sessionEstablished: false,
    });
    const incoming = run({
      projectId: 'project-1',
      taskId: 'task-004',
      taskUuid: 'new-task-uuid',
    }, [newAttempt]);

    const merged = mergeConversationRunSnapshot(current, incoming, 'create');
    const mergedLeaf = merged.sessionTree.rounds[0].nodes[0].attempts[0];

    expect(merged).not.toHaveProperty('selectedSession');
    expect(mergedLeaf.sessionId).toBeNull();
    expect(mergedLeaf.sessionEstablished).toBe(false);
  });

  it('does not let an initial unknown ACP snapshot downgrade a running runtime leaf', () => {
    const current = run();
    const incoming = {
      ...withLeaf(current, leaf('unknown', unknownDisplay)),
      activeSessions: [],
    };

    const merged = mergeConversationRunSnapshot(current, incoming, 'initial-load');
    const mergedLeaf = merged.sessionTree.rounds[0].nodes[0].attempts[0];

    expect(mergedLeaf.status).toBe('running');
    expect(mergedLeaf.runtimeDisplay.tone).toBe('running');
    expect(merged.activeSessions).toHaveLength(1);
    expect(merged).not.toHaveProperty('selectedSession');
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

  it('preserves manual selection without introducing an ACP content field', () => {
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
    expect(merged).not.toHaveProperty('selectedSession');
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
      sessionTree: {
        ...run({}, [testAttempt, devAttempt]).sessionTree,
        selectedSessionKey: 'round-001/test/attempt-001',
      },
    }, [testAttempt, devAttempt]);

    const merged = mergeConversationRunSnapshot(current, incoming, 'live-refresh', {
      selectedSessionKey: 'round-001/dev/attempt-002',
    });

    expect(merged.sessionTree.selectedSessionKey).toBe('round-001/dev/attempt-002');
    expect(merged).not.toHaveProperty('selectedSession');
  });

  it('keeps same-key navigation and paused lifecycle without ACP content', () => {
    const current = run({
      runStatus: 'paused',
    });
    const incoming = run({
      runStatus: 'paused',
      activeSessions: [],
      sessionTree: {
        ...current.sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-001',
      },
    });

    const merged = mergeConversationRunSnapshot(current, incoming, 'live-refresh');

    expect(merged.sessionTree.selectedSessionKey).toBe('round-001/dev/attempt-001');
    expect(merged.runStatus).toBe('paused');
    expect(merged).not.toHaveProperty('selectedSession');
  });

  it('does not downgrade an established session reference during a same-attempt resume refresh', () => {
    const establishedLeaf = leaf('completed', runningDisplay, {
      current: true,
      sessionId: 'session-1',
      sessionEstablished: true,
    });
    const transientLeaf = leaf('running', runningDisplay, {
      current: true,
      sessionId: null,
      sessionEstablished: false,
    });
    const current = run({
      runStatus: 'completed',
    }, [establishedLeaf]);
    const incoming = run({
      runStatus: 'running',
    }, [transientLeaf]);

    const merged = mergeConversationRunSnapshot(current, incoming, 'live-refresh');
    const mergedLeaf = merged.sessionTree.rounds[0].nodes[0].attempts[0];

    expect(mergedLeaf.sessionEstablished).toBe(true);
    expect(mergedLeaf.sessionId).toBe('session-1');
    expect(merged).not.toHaveProperty('selectedSession');
  });

  it('applies a same-key run summary without introducing ACP content', () => {
    const current = run({
      runStatus: 'paused',
    });
    const incoming = run({
      runStatus: 'paused',
      activeSessions: [],
      sessionTree: {
        ...current.sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-001',
      },
    });

    const merged = mergeConversationRunSnapshot(current, incoming, 'live-refresh');

    expect(merged.sessionTree.selectedSessionKey).toBe('round-001/dev/attempt-001');
    expect(merged.runStatus).toBe('paused');
    expect(merged).not.toHaveProperty('selectedSession');
  });

  it('switches selected navigation identity without introducing ACP content', () => {
    const devAttempt = leaf('completed', runningDisplay, { nodeId: 'dev', attemptId: 'attempt-001', current: false });
    const testAttempt = leaf('running', runningDisplay, { nodeId: 'test', attemptId: 'attempt-001', current: true });
    const current = run({
      sessionTree: {
        ...run({}, [devAttempt, testAttempt]).sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-001',
      },
    }, [devAttempt, testAttempt]);
    const incoming = run({
      sessionTree: {
        ...run({}, [devAttempt, testAttempt]).sessionTree,
        selectedSessionKey: 'round-001/test/attempt-001',
      },
    }, [devAttempt, testAttempt]);

    const merged = mergeConversationRunSnapshot(current, incoming, 'live-refresh');

    expect(merged.sessionTree.selectedSessionKey).toBe('round-001/test/attempt-001');
    expect(merged).not.toHaveProperty('selectedSession');
  });

  it('honors the preferred selection instead of the incoming selected identity', () => {
    const acceptAttempt = leaf('running', runningDisplay, { nodeId: 'accept', attemptId: 'attempt-001', current: true });
    const devAttempt = leaf('completed', runningDisplay, { nodeId: 'dev', attemptId: 'attempt-002', current: false });
    const current = run({
      sessionTree: {
        ...run({}, [acceptAttempt, devAttempt]).sessionTree,
        selectedSessionKey: 'round-001/accept/attempt-001',
      },
    }, [acceptAttempt, devAttempt]);
    const incoming = run({
      sessionTree: {
        ...run({}, [acceptAttempt, devAttempt]).sessionTree,
        selectedSessionKey: 'round-001/dev/attempt-002',
      },
    }, [acceptAttempt, devAttempt]);

    const merged = mergeConversationRunSnapshot(current, incoming, 'live-refresh', {
      selectedSessionKey: 'round-001/accept/attempt-001',
    });

    expect(merged.sessionTree.selectedSessionKey).toBe('round-001/accept/attempt-001');
    expect(merged).not.toHaveProperty('selectedSession');
  });

  it('replaces state when the snapshot belongs to a different run', () => {
    const current = run();
    const incoming = run({ runId: 'run-002', sessionTree: { rounds: [], selectedSessionKey: null } });

    const merged = mergeConversationRunSnapshot(current, incoming, 'rerun');

    expect(merged.runId).toBe('run-002');
    expect(merged.sessionTree.rounds).toHaveLength(0);
  });
});
