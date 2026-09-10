import { test } from 'node:test';
import assert from 'node:assert/strict';
import { archiveDynamicRelations, projectArchiveRun } from './project-demo-run.mjs';

test('derives dynamic relations from accepted source control and distinguishes continuation', () => {
  const graph = { nodes: [{ id: 'a' }, { id: 'b', dependsOn: ['a', 'missing'], sessionMode: 'continue', continueFromNodeId: 'a' }, { id: 'c' }],
    proposals: [{ validationStatus: 'accepted', sourceNodeId: 'a', parsed: { next: { type: 'single', node: { id: 'b' } } } },
      { validationStatus: 'rejected', sourceNodeId: 'b', parsed: { next: { type: 'single', node: { id: 'c' } } } }],
    groups: [{ createdByNodeId: 'b', rootNodeIds: ['c', 'b'] }] };
  assert.deepEqual(archiveDynamicRelations(graph).map(({ from, to, label }) => ({ from, to, label })), [
    { from: 'a', to: 'b', label: 'depends-on' }, { from: 'a', to: 'b', label: 'continue' }, { from: 'b', to: 'c', label: 'success' },
  ]);
});

test('projects a paused source run with exact nested attempts and no fabricated completion', () => {
  const source = { id: 'n', title: 'Original node', status: 'paused', outcome: null, startedAt: '1Z' };
  const manifest = { projectId: 'p', task: { id: 't', title: 'Original task' },
    run: { id: 'r', status: 'paused', outcome: null, pause_reason: 'process-interrupted', current_round: 'round', current_node: 'outer' },
    sessions: [{ roundId: 'round', outerNodeId: 'outer', outerAttemptId: 'outer-attempt', nodeId: 'n', attemptId: 'source-attempt', node: source }] };
  const graph = { run: { status: 'paused', outcome: null, currentNodeIds: ['n'] }, nodes: [{ ...source, kind: 'agent-task' }], groups: [], proposals: [] };
  const run = projectArchiveRun(manifest, [{ id: 'round', index: 1, status: 'paused', outcome: null }],
    [{ roundId: 'round', nodeId: 'outer', attemptId: 'outer-attempt', graph }]);
  assert.equal(run.runStatus, 'paused');
  assert.equal(run.runOutcome, null);
  assert.equal(run.pauseReason, 'process-interrupted');
  assert.equal(run.sessionTree.selectedSessionKey, 'round/outer/outer-attempt/n/source-attempt');
  assert.equal(run.sessionTree.rounds[0].nodes[0].outerNodes[0].attempts[0].startedAt, '1970-01-01T00:00:01.000Z');
  assert.equal(run.workflowGraph.nodes[0].attemptId, 'source-attempt');
  assert.equal(run.workflowGraph.nodes[0].runtimeDisplay.terminal, false);
  assert.equal(run.selectedSession, null);
  assert.deepEqual(run.activeSessions, []);
});
