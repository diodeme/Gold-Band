import type { ConversationRunVm, ConversationSessionTargetVm } from '@/types';

const excluded = {
  projectId: 'e-projects-code-ai-ji--a40d4379', taskId: 'task-015', runId: 'run-001',
  roundId: 'round-001', outerNodeId: 'ai-dynamic', outerAttemptId: 'attempt-001',
  nodeId: 'phase-2-parallel-capabilities-merge-2', attemptId: 'attempt-001',
};
type Scope = Pick<ConversationRunVm, 'projectId' | 'taskId' | 'runId'>;
export function includesDemoSession(scope: Scope, session: ConversationSessionTargetVm) {
  return !Object.entries(excluded).every(([key, value]) => ({ ...scope, ...session } as Record<string, unknown>)[key] === value);
}
const sessionKey = (leaf: ConversationSessionTargetVm) => leaf.outerNodeId
  ? `${leaf.roundId}/${leaf.outerNodeId}/${leaf.outerAttemptId}/${leaf.nodeId}/${leaf.attemptId}`
  : `${leaf.roundId}/${leaf.nodeId}/${leaf.attemptId}`;

export function selectDemoSessions(run: ConversationRunVm): ConversationRunVm {
  const rounds = run.sessionTree.rounds.map(round => ({ ...round, nodes: round.nodes.map(node => ({
    ...node, attempts: node.attempts.filter(leaf => includesDemoSession(run, leaf)),
    outerNodes: node.outerNodes?.map(outer => ({ ...outer, attempts: outer.attempts.filter(leaf => includesDemoSession(run, leaf)) })).filter(outer => outer.attempts.length),
  })).filter(node => node.attempts.length || node.outerNodes?.length) }));
  const leaves = rounds.flatMap(round => round.nodes.flatMap(node => [...node.attempts, ...(node.outerNodes ?? []).flatMap(outer => outer.attempts)]));
  const selected = leaves.find(leaf => sessionKey(leaf) === run.sessionTree.selectedSessionKey) ?? leaves.at(-1);
  return { ...run, sessionTree: { ...run.sessionTree, rounds, selectedSessionKey: selected ? sessionKey(selected) : null } };
}
