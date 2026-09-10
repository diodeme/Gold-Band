import type { ConversationRunVm, ConversationTreeNodeVm } from '@/types';

// Editorial exclusions apply only to the browser Demo, never to source archives.
export const excludedDemoSessions = [{
  projectId: 'e-projects-code-ai-ji--a40d4379', taskId: 'task-015', runId: 'run-001',
  roundId: 'round-001', outerNodeId: 'ai-dynamic', outerAttemptId: 'attempt-001',
  nodeId: 'phase-2-parallel-capabilities-merge-2', attemptId: 'attempt-001',
}] as const;

export function isExcludedDemoSession(locator: object) {
  return excludedDemoSessions.some(excluded => Object.entries(excluded).every(([key, value]) => Reflect.get(locator, key) === value));
}

export function selectVisibleDemoSessions(run: ConversationRunVm): ConversationRunVm {
  const removed = new Set<string>();
  let firstVisible: string | null = null;
  const nodes = (items: ConversationTreeNodeVm[]): ConversationTreeNodeVm[] => items.flatMap(node => {
    const attempts = node.attempts.filter(attempt => {
      if (!isExcludedDemoSession({ ...run, ...attempt })) return true;
      removed.add(attempt.pathLabel);
      return false;
    });
    const outerNodes = node.outerNodes ? nodes(node.outerNodes) : undefined;
    firstVisible ??= attempts[0]?.pathLabel ?? null;
    return attempts.length || outerNodes?.length ? [{ ...node, attempts, outerNodes }] : [];
  });
  const rounds = run.sessionTree.rounds.map(round => ({ ...round, nodes: nodes(round.nodes) }));
  return { ...run, sessionTree: { ...run.sessionTree, rounds,
    selectedSessionKey: removed.has(run.sessionTree.selectedSessionKey ?? '') ? firstVisible : run.sessionTree.selectedSessionKey } };
}
