import type { ConversationRunVm, ConversationSessionLeafVm, ConversationSessionTreeVm, GraphNodeVm } from '../types';

type RuntimeWorkflowRun = Pick<ConversationRunVm, 'runMode' | 'workflowGraph'>;
type RuntimeWorkflowLeaf = Pick<ConversationSessionLeafVm, 'outerNodeId' | 'outerAttemptId'>;

export function isAiDynamicInnerSession(leaf?: RuntimeWorkflowLeaf | null): boolean {
  return Boolean(leaf?.outerNodeId && leaf?.outerAttemptId);
}

export function canViewConversationRuntimeWorkflow(
  run: RuntimeWorkflowRun,
  selectedLeaf?: RuntimeWorkflowLeaf | null,
): boolean {
  if (run.runMode === 'workflow') return true;
  return isAiDynamicInnerSession(selectedLeaf) && run.workflowGraph.nodes.length > 0;
}

export function conversationSessionLeafForGraphNode(
  tree: ConversationSessionTreeVm,
  graphNode: Pick<GraphNodeVm, 'nodeId' | 'attemptId' | 'outerNodeId' | 'outerAttemptId'>,
): ConversationSessionLeafVm | null {
  const nodeId = graphNode.nodeId;
  if (!nodeId) return null;

  for (let r = tree.rounds.length - 1; r >= 0; r--) {
    const round = tree.rounds[r];
    for (const node of round.nodes) {
      if (graphNode.outerNodeId && graphNode.outerAttemptId) {
        if (node.nodeId !== graphNode.outerNodeId) continue;
        for (const outerNode of node.outerNodes ?? []) {
          if (outerNode.nodeId !== nodeId) continue;
          const leaf = matchingAttempt(outerNode.attempts, graphNode.attemptId, graphNode.outerNodeId, graphNode.outerAttemptId);
          if (leaf) return leaf;
        }
        continue;
      }

      if (node.nodeId !== nodeId) continue;
      const leaf = matchingAttempt(node.attempts, graphNode.attemptId);
      if (leaf) return leaf;
    }
  }

  return null;
}

function matchingAttempt(
  attempts: ConversationSessionLeafVm[],
  attemptId?: string | null,
  outerNodeId?: string | null,
  outerAttemptId?: string | null,
) {
  const scopedAttempts = outerNodeId && outerAttemptId
    ? attempts.filter((attempt) => attempt.outerNodeId === outerNodeId && attempt.outerAttemptId === outerAttemptId)
    : attempts;
  if (attemptId) {
    const exact = scopedAttempts.find((attempt) => attempt.attemptId === attemptId);
    if (exact) return exact;
  }
  return scopedAttempts[scopedAttempts.length - 1] ?? null;
}
