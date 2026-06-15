import type { ConversationRunVm, ConversationSessionLeafVm } from '../types';

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
