import { describe, expect, it } from 'vitest';
import { includesDemoSession, selectDemoSessions } from '../../marketing/demo/session-selection';
import { createDatasetReader } from '../../marketing/demo/dataset';
import { mockErrorBlockedConversationRun } from '../src/mockData';

const scope = { projectId: 'e-projects-code-ai-ji--a40d4379', taskId: 'task-015', runId: 'run-001' };
const removed = { roundId: 'round-001', outerNodeId: 'ai-dynamic', outerAttemptId: 'attempt-001', nodeId: 'phase-2-parallel-capabilities-merge-2', attemptId: 'attempt-001', pathLabel: '' };
describe('Demo conversation selection', () => {
  it('excludes only the selected full locator', () => {
    expect(includesDemoSession(scope, removed)).toBe(false);
    expect(includesDemoSession({ ...scope, taskId: 'another' }, removed)).toBe(true);
    expect(includesDemoSession(scope, { ...removed, attemptId: 'attempt-002' })).toBe(true);
  });
  it('removes the menu leaf and selects another without changing source state', () => {
    const run = structuredClone(mockErrorBlockedConversationRun);
    Object.assign(run, scope);
    const round = run.sessionTree.rounds[0];
    const node = round.nodes[0];
    const leaf = { ...node.attempts[0], ...removed };
    node.attempts = [leaf];
    node.outerNodes = undefined;
    round.nodes = [node, { ...node, nodeId: 'remaining', attempts: [{ ...leaf, nodeId: 'remaining' }] }];
    run.sessionTree.rounds = [round];
    run.sessionTree.selectedSessionKey = 'round-001/ai-dynamic/attempt-001/phase-2-parallel-capabilities-merge-2/attempt-001';
    const projected = selectDemoSessions(run);
    expect(projected.sessionTree.rounds[0].nodes).toHaveLength(1);
    expect(projected.sessionTree.selectedSessionKey).toBe('round-001/ai-dynamic/attempt-001/remaining/attempt-001');
    expect(run.sessionTree.rounds[0].nodes).toHaveLength(2);
    expect(projected.runStatus).toBe(run.runStatus);
  });
  it('rejects deep-link session reads before loading excluded content', async () => {
    const requests: string[] = [];
    const reader = createDatasetReader('/data/', async input => {
      requests.push(String(input));
      return new Response(JSON.stringify({ version: 1, ...scope, sessions: [{ ...removed, branchId: 'root', path: 'excluded/index.json' }] }));
    });
    await expect(reader.api.getAcpSession(scope.projectId, scope.taskId, scope.runId, removed.roundId, removed.nodeId, removed.attemptId, undefined, undefined, removed.outerNodeId, removed.outerAttemptId)).rejects.toMatchObject({ code: 'demo.resource-not-found' });
    expect(requests).toEqual(['/data/dataset.json']);
    expect((await reader.dataset()).sessions).toEqual([]);
  });
});
