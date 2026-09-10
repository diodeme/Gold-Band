import { describe, expect, it } from 'vitest';
import { mockErrorBlockedConversationRun } from '../src/mockData';
import { createWorkflowScene, showcaseWorkflow, WORKFLOW_STEPS } from '../../marketing/site/workflow-scene';

describe('recording workflow uses one scenario for graph and session', () => {
  it('keeps each node history scoped to its owner', () => {
    const repair = createWorkflowScene(mockErrorBlockedConversationRun, 'en', 'repair');
    expect(repair.selectedSession?.events.some(event => event.content?.includes('"result": false'))).toBe(false);
    const review = createWorkflowScene(mockErrorBlockedConversationRun, 'en', 'repair', 'review');
    expect(review.selectedSession?.events.at(-1)?.content).toContain('"result": false');
    expect(review.selectedSession?.nodeId).toBe('review');
  });
  it('keeps terminal composer consistent with the selected attempt', () => {
    for (const step of ['false', 'true', 'complete'] as const) {
      const run = createWorkflowScene(mockErrorBlockedConversationRun, 'en', step);
      const leaf = run.sessionTree.rounds[0].nodes.find(node => node.nodeId === run.selectedSession?.nodeId)!.attempts[0];
      expect(run.selectedSession?.status).toBe(leaf.status);
      expect(leaf.lifecycle?.composer.canStop).toBe(false);
      expect(leaf.lifecycle?.acp.latestTurnStatus).toBe('completed');
      expect(run.activeSessions).toHaveLength(0);
    }
  });
  it('shows the false result before entering repair and true before delivery', () => {
    const failed = createWorkflowScene(mockErrorBlockedConversationRun, 'en', 'false');
    expect(failed.selectedSession?.nodeId).toBe('review');
    expect(failed.selectedSession?.events.at(-1)?.content).toContain('"result": false');
    expect(failed.workflowGraph.nodes.find(n => n.nodeId === 'review')?.outcome).toBe('failure');
    expect(failed.workflowGraph.nodes.find(n => n.nodeId === 'repair')?.status).toBe('pending');
    expect(createWorkflowScene(mockErrorBlockedConversationRun, 'en', 'repair').selectedSession?.nodeId).toBe('repair');
    const passed = createWorkflowScene(mockErrorBlockedConversationRun, 'en', 'true');
    expect(passed.selectedSession?.nodeId).toBe('repair');
    expect(passed.selectedSession?.events.at(-1)?.content).toContain('"result": true');
    expect(passed.workflowGraph.nodes.find(n => n.nodeId === 'deliver')?.status).toBe('pending');
    expect(createWorkflowScene(mockErrorBlockedConversationRun, 'en', 'deliver').selectedSession?.nodeId).toBe('deliver');
    expect(showcaseWorkflow.edges).toContainEqual({ from: 'review', to: 'repair', on: 'failure' });
  });
  it('preserves graph and lifecycle agreement at every bilingual checkpoint', () => {
    for (const language of ['en', 'zh'] as const) for (const step of WORKFLOW_STEPS) {
      const run = createWorkflowScene(mockErrorBlockedConversationRun, language, step);
      for (const graph of run.workflowGraph.nodes) {
        const leaf = run.sessionTree.rounds[0].nodes.find(n => n.nodeId === graph.nodeId)!.attempts[0];
        expect(graph.status).toBe(leaf.lifecycle?.runtime.status);
        expect(graph.outcome).toBe(leaf.lifecycle?.runtime.outcome);
        expect(graph.runtimeDisplay).toEqual(leaf.runtimeDisplay);
      }
      expect(run.selectedSession?.pendingInteractions.length).toBe(step === 'permission' || step === 'question' ? 1 : 0);
      expect(run.runOutcome).toBe(step === 'complete' ? 'success' : null);
    }
  });
});
