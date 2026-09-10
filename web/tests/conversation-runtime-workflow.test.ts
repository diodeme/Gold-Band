import { describe, expect, it } from 'vitest';
import { canViewConversationRuntimeWorkflow, conversationSessionLeafForGraphNode, isAiDynamicInnerSession, runtimeGraphNodeForSession } from '../src/lib/conversation-runtime-workflow';
import type { ConversationRunVm, ConversationSessionLeafVm, ConversationSessionTreeVm, GraphVm, RuntimeDisplayVm } from '../src/types';

const successDisplay: RuntimeDisplayVm = {
  code: 'success',
  tone: 'success',
  icon: 'check',
  terminal: true,
  blockingError: false,
  resumable: false,
};

const emptyGraph: GraphVm = { nodes: [], edges: [] };

describe('runtime graph session focus', () => {
  it('resolves the exact attempt and outer scope without name fallback', () => {
    const nodes = [
      { id: 'outer-a', nodeId: 'accept', outerNodeId: 'group-a', outerAttemptId: 'outer-1', attemptId: 'attempt-1' },
      { id: 'outer-b', nodeId: 'accept', outerNodeId: 'group-b', outerAttemptId: 'outer-1', attemptId: 'attempt-1' },
      { id: 'retry', nodeId: 'accept', outerNodeId: 'group-b', outerAttemptId: 'outer-1', attemptId: 'attempt-2' },
    ] as GraphVm['nodes'];
    const session = { nodeId: 'accept', outerNodeId: 'group-b', outerAttemptId: 'outer-1', attemptId: 'attempt-2' };
    expect(runtimeGraphNodeForSession({ nodes, edges: [] }, session)?.id).toBe('retry');
    expect(runtimeGraphNodeForSession({ nodes, edges: [] }, { ...session, outerAttemptId: 'missing' })).toBeNull();
    expect(runtimeGraphNodeForSession({ nodes: [nodes[2], { ...nodes[2], id: 'duplicate' }], edges: [] }, session)).toBeNull();
    expect(runtimeGraphNodeForSession({ nodes, edges: [] }, null)).toBeNull();
  });
});
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
      runtime: { status: 'completed', outcome: 'success', pauseReason: null, resumable: false, current: false, active: false, continuable: false, phase: 'terminal' },
      control: { mode: 'non-runtime-controlled' },
      acp: { sessionAvailability: 'established', liveTurnActivity: 'idle', latestTurnStatus: 'completed', stopping: false },
      displayStatus: 'success',
      runtimeDisplay: successDisplay,
      continueKind: null,
      composer: {
        mode: 'normal',
        submitTarget: 'acp-prompt',
        processingKind: 'processing',
        statusKey: null,
        canStop: false,
        lockInput: false,
      },
    },
    current: false,
    manualCheckPending: false,
    artifactCount: 0,
    attachmentCount: 0,
    ...overrides,
  };
}

function tree(): ConversationSessionTreeVm {
  const topAttempt = leaf({
    nodeId: 'review',
    attemptId: 'attempt-002',
    pathLabel: 'review/attempt-002',
  });
  const dynamicAttempt = leaf({
    nodeId: 'bootstrap',
    attemptId: 'attempt-001',
    outerNodeId: 'ai-dynamic',
    outerAttemptId: 'attempt-001',
    pathLabel: 'bootstrap/attempt-001',
  });
  return {
    selectedSessionKey: null,
    rounds: [{
      roundId: 'round-001',
      index: 1,
      label: 'round-001',
      status: 'completed',
      runtimeDisplay: successDisplay,
      nodes: [
        {
          nodeId: 'review',
          label: 'Review',
          nodeType: 'worker',
          status: 'completed',
          runtimeDisplay: successDisplay,
          attempts: [topAttempt],
        },
        {
          nodeId: 'ai-dynamic',
          label: 'AI Dynamic',
          nodeType: 'ai-dynamic',
          status: 'completed',
          runtimeDisplay: successDisplay,
          attempts: [],
          outerNodes: [{
            nodeId: 'bootstrap',
            label: 'AI-DYNAMIC bootstrap',
            nodeType: 'dynamic-bootstrap',
            status: 'completed',
            runtimeDisplay: successDisplay,
            attempts: [dynamicAttempt],
          }],
        },
      ],
    }],
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

  it('resolves top-level workflow graph nodes to their session leaf', () => {
    const resolved = conversationSessionLeafForGraphNode(tree(), {
      nodeId: 'review',
      attemptId: 'attempt-002',
    });

    expect(resolved?.pathLabel).toBe('review/attempt-002');
  });

  it('resolves AI-DYNAMIC internal graph nodes by outer attempt scope', () => {
    const resolved = conversationSessionLeafForGraphNode(tree(), {
      nodeId: 'bootstrap',
      attemptId: 'attempt-001',
      outerNodeId: 'ai-dynamic',
      outerAttemptId: 'attempt-001',
    });

    expect(resolved?.pathLabel).toBe('bootstrap/attempt-001');
    expect(resolved?.outerNodeId).toBe('ai-dynamic');
    expect(resolved?.outerAttemptId).toBe('attempt-001');
  });
});
