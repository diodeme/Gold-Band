import { expect, it } from 'vitest';
import { excludedDemoSessions, isExcludedDemoSession, selectVisibleDemoSessions } from '../../marketing/demo/session-selection';
import { archiveSession } from '../../marketing/demo/archive';
import { projectArchiveRun } from '../../scripts/project-demo-run.mjs';

it('removes only the chosen attempt and clears its selection without changing source data', () => {
  const locator = excludedDemoSessions[0];
  const sibling = { ...locator, nodeId: 'phase-2-parallel-capabilities-merge' };
  const sessions = [locator, sibling].map(item => ({ ...item, node: { id: item.nodeId, title: 'Same title', status: 'paused', outcome: null } }));
  const catalog = { projectId: locator.projectId, task: { id: locator.taskId }, run: { id: locator.runId, status: 'paused', outcome: null }, sessions };
  const run = projectArchiveRun(catalog, [{ id: locator.roundId, index: 1, status: 'paused', outcome: null }], [{
    roundId: locator.roundId, nodeId: locator.outerNodeId, attemptId: locator.outerAttemptId,
    graph: { nodes: [], run: { status: 'paused', outcome: null, currentNodeIds: [] } },
  }]);
  const path = `${locator.roundId}/${locator.outerNodeId}/${locator.outerAttemptId}/${locator.nodeId}/${locator.attemptId}`;
  run.sessionTree.selectedSessionKey = path;
  const visible = selectVisibleDemoSessions(run);
  expect(JSON.stringify(visible.sessionTree)).not.toContain(locator.nodeId);
  expect(JSON.stringify(visible.sessionTree)).toContain(sibling.nodeId);
  expect(visible.sessionTree.selectedSessionKey).toBe(path.replace(locator.nodeId, sibling.nodeId));
  expect(run.sessionTree.selectedSessionKey).toBe(path);
  expect(visible.workflowGraph).toBe(run.workflowGraph);
  for (const key of Object.keys(locator)) expect(isExcludedDemoSession({ ...locator, [key]: 'other' })).toBe(false);
  expect(() => archiveSession({ sessions } as never, locator)).toThrow();
});
