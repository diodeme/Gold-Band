import { describe, expect, it } from 'vitest';
import { canViewConversationRuntimeWorkflow, isAiDynamicInnerSession } from '../src/lib/conversation-runtime-workflow';
import type { ConversationRunVm, ConversationSessionLeafVm, GraphVm, RuntimeDisplayVm } from '../src/types';

const successDisplay: RuntimeDisplayVm = {
  code: 'success',
  tone: 'success',
  icon: 'check',
  terminal: true,
  blockingError: false,
  resumable: false,
};

const emptyGraph: GraphVm = { nodes: [], edges: [] };
const runtimeGraph: GraphVm = {
  nodes: [{
    id: 'ai-dynamic::attempt-001::bootstrap',
    nodeId: 'bootstrap',
    sequence: 1,
    label: 'AI-DYNAMIC bootstrap',
    nodeType: 'dynamic-bootstrap',
    runtimeDisplay: successDisplay,
    attemptCount: 1,
    attempts: [],
    artifactCount: 0,
    attachmentCount: 0,
    current: false,
    outerNodeId: 'ai-dynamic',
    outerAttemptId: 'attempt-001',
  }],
  edges: [],
};

function run(runMode: ConversationRunVm['runMode'], workflowGraph = emptyGraph) {
  return { runMode, workflowGraph };
}

function leaf(overrides: Partial<ConversationSessionLeafVm> = {}) {
  return {
    roundId: 'round-001',
    nodeId: 'bootstrap',
    attemptId: 'attempt-001',
    pathLabel: 'bootstrap/attempt-001',
    status: 'completed',
    runtimeDisplay: successDisplay,
    lifecycle: {
      runtime: { status: 'completed', outcome: 'success', pauseReason: null, resumable: false, current: false, active: false, continuable: false },
      acp: { status: 'completed', active: false, stopping: false, terminal: true },
      displayStatus: 'success',
      runtimeDisplay: successDisplay,
      continueKind: null,
    },
    current: false,
    artifactCount: 0,
    attachmentCount: 0,
    ...overrides,
  };
}

describe('conversation runtime workflow actions', () => {
  it('keeps workflow runs viewable even before a runtime graph is available', () => {
    expect(canViewConversationRuntimeWorkflow(run('workflow'), null)).toBe(true);
  });

  it('allows AUTO AI-DYNAMIC inner sessions to view the runtime workflow graph', () => {
    const selectedLeaf = leaf({ outerNodeId: 'ai-dynamic', outerAttemptId: 'attempt-001' });

    expect(isAiDynamicInnerSession(selectedLeaf)).toBe(true);
    expect(canViewConversationRuntimeWorkflow(run('auto', runtimeGraph), selectedLeaf)).toBe(true);
  });

  it('does not expose a workflow viewer for AUTO sessions without a dynamic runtime graph', () => {
    const selectedLeaf = leaf({ outerNodeId: 'ai-dynamic', outerAttemptId: 'attempt-001' });

    expect(canViewConversationRuntimeWorkflow(run('auto'), selectedLeaf)).toBe(false);
    expect(canViewConversationRuntimeWorkflow(run('auto', runtimeGraph), leaf())).toBe(false);
  });
});
