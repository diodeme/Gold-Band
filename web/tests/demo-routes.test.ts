import { describe, expect, it } from 'vitest';
import { demoLinkParameters, demoPageFromHash, demoRunModeFromHash } from '../../marketing/demo/routes';

describe('demo deep links', () => {
  it('restores a historical run and preserves the round parameter', () => {
    expect(demoPageFromHash('#demo-review?run=run-051&round=round-002&node=dev-test')).toMatchObject({ runId: 'run-051' });
    expect(demoLinkParameters('#demo-review?run=run-051&round=round-002&node=dev-test').get('round')).toBe('round-002');
    expect(demoPageFromHash('#mock-task?run=run-051')).toMatchObject({ runId: 'run-051' });
  });
  it('restores the requested run mode on initial load', () => {
    expect(demoRunModeFromHash('#conversation-home?mode=workflow').mode).toBe('workflow');
    expect(demoRunModeFromHash('#run-mode-management?template=default-lightweight')).toMatchObject({ mode: 'workflow', workflowTemplateId: 'default-lightweight' });
    expect(demoRunModeFromHash('#conversation-home?mode=auto').mode).toBe('auto');
    expect(demoRunModeFromHash('#conversation-home?mode=direct').mode).toBe('direct');
    expect(demoRunModeFromHash('#conversation-home?mode=unknown').mode).toBe('direct');
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
  });
});
