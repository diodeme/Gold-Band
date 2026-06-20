import { describe, expect, it } from 'vitest';
import { canViewConversationRuntimeWorkflow, conversationSessionLeafForGraphNode, isAiDynamicInnerSession } from '../src/lib/conversation-runtime-workflow';
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
      acp: { status: 'completed', active: false, stopping: false, terminal: true },
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
        showContinueAction: false,
      },
    },
    current: false,
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
