import { describe, expect, it } from 'vitest';
import { demoLinkParameters, demoPageFromHash, demoRunModeFromHash, demoSessionLink, selectDemoSession } from '../../marketing/demo/routes';
import { createDemoApi } from '../../marketing/demo/api';

describe('demo deep links', () => {
  it('resolves exact session links and rejects missing or ambiguous selections', async () => {
    const run = await createDemoApi().getConversationRun('default', 'demo-review', 'run-052');
    const leaf = run.sessionTree.rounds[0].nodes[0].attempts[0];
    const link = demoSessionLink(run.projectId, run.taskId, run.runId, leaf);
    expect(selectDemoSession(run, demoLinkParameters(link))).toEqual(leaf);
    expect(selectDemoSession(run, new URLSearchParams()).nodeId).toBe(run.selectedSession!.nodeId);
    for (const query of ['round=missing&node=accept', 'node=accept', 'round=round-001&node=accept&attempt=missing', 'outerNode=wrong']) {
      expect(() => selectDemoSession(run, new URLSearchParams(query))).toThrow();
    }
    const nested = { ...leaf, outerNodeId: 'outer', outerAttemptId: 'outer-attempt' };
    run.sessionTree.rounds[0].nodes[0].outerNodes = [{ ...run.sessionTree.rounds[0].nodes[0], attempts: [nested] }];
    expect(selectDemoSession(run, demoLinkParameters(demoSessionLink(run.projectId, run.taskId, run.runId, nested)))).toEqual(nested);
    expect(selectDemoSession(run, demoLinkParameters(link))).toEqual(leaf);
  });
  it('restores a historical run and preserves the round parameter', () => {
    expect(demoPageFromHash('#demo-review?run=run-051&round=round-002&node=dev-test')).toMatchObject({ runId: 'run-051' });
    expect(demoLinkParameters('#demo-review?run=run-051&round=round-002&node=dev-test').get('round')).toBe('round-002');
    expect(demoPageFromHash('#mock-task?run=run-051')).toMatchObject({ runId: 'run-051' });
    expect(demoPageFromHash('#mock-task?project=another-project')).toMatchObject({ projectId: 'another-project' });
  });
  it('restores the requested run mode on initial load', () => {
    expect(demoRunModeFromHash('#conversation-home?mode=workflow').mode).toBe('workflow');
    expect(demoRunModeFromHash('#run-mode-management?template=default-lightweight')).toMatchObject({ mode: 'workflow', workflowTemplateId: 'default-lightweight' });
    expect(demoRunModeFromHash('#conversation-home?mode=auto').mode).toBe('auto');
    expect(demoRunModeFromHash('#conversation-home?mode=direct').mode).toBe('direct');
    expect(demoRunModeFromHash('#conversation-home?mode=unknown').mode).toBe('direct');
  });
  it('applies history route selection without discarding the current temporary configuration', () => {
    const current = { mode: 'auto' as const, autoConfig: { agentType: 'claude-acp', activeTemplateId: 'demo-project-auto' },
      directConfig: { agentType: 'codex-acp' }, workflowTemplateId: 'default-lightweight' };
    expect(demoRunModeFromHash('#run-mode-management?mode=auto', current)).toEqual(current);
    expect(demoRunModeFromHash('#conversation-home?mode=direct', current)).toEqual({ ...current, mode: 'direct' });
    expect(demoRunModeFromHash('#run-mode-management?template=default', current)).toEqual({ ...current, mode: 'workflow', workflowTemplateId: 'default' });
    expect(demoRunModeFromHash('#conversation-home?mode=auto').autoConfig).toBeUndefined();
  });
  it('resolves every shared page and both preset conversations', () => {
    for (const kind of ['contexts', 'settings', 'agents', 'conversation-home', 'run-mode-management']) {
      expect(demoPageFromHash(`#${kind}`)).toEqual({ kind });
    }
    expect(demoPageFromHash('#mock-task')).toMatchObject({ kind: 'conversation-run', taskId: 'mock-task' });
    expect(demoPageFromHash('#demo-review?node=dev-test')).toMatchObject({ kind: 'conversation-run', taskId: 'demo-review' });
    expect(demoLinkParameters('#demo-review?node=dev-test').get('node')).toBe('dev-test');
    expect(demoLinkParameters('#contexts?tab=mcp').get('tab')).toBe('mcp');
    expect(demoLinkParameters('#run-mode-management?template=default-lightweight').get('template')).toBe('default-lightweight');
    expect(demoPageFromHash('#unknown')).toMatchObject({ taskId: 'unknown' });
    expect(demoPageFromHash('')).toMatchObject({ taskId: 'mock-task' });
  });
});
