import { describe, expect, it } from 'vitest';

import { hasWorkflowDraftChanges, shouldShowDefaultWorkflowSaveAsNotice, workflowTemplateDisplayName } from '@/lib/workflow-template';

const defaultTemplate = {
  id: 'default',
  name: '默认工作流',
  workflow: { version: '0.1', id: 'default-workflow', entry: '', control: {}, nodes: [], edges: [] },
  createdAt: '2026-08-07T00:00:00Z',
  updatedAt: '2026-08-07T00:00:00Z',
};

describe('workflow template display names', () => {
  it('localizes the built-in template from its stable ID instead of its persisted name', () => {
    const t = (key: string) => key === 'taskList.create.defaultWorkflow' ? 'Default workflow' : key;
    expect(workflowTemplateDisplayName(defaultTemplate, t)).toBe('Default workflow');
  });

  it('preserves user-defined template names', () => {
    const t = (key: string) => key;
    expect(workflowTemplateDisplayName({ ...defaultTemplate, id: 'custom', name: 'Release checklist' }, t)).toBe('Release checklist');
  });

  it('shows the save-as notice only after the built-in default workflow changes', () => {
    const baseline = { ...defaultTemplate.workflow, entry: 'plan' };
    const changed = { ...baseline, entry: 'dev' };
    const restored = { ...baseline, model: undefined, config_options: undefined };

    expect(hasWorkflowDraftChanges(changed, baseline)).toBe(true);
    expect(shouldShowDefaultWorkflowSaveAsNotice('default', changed, baseline)).toBe(true);
    expect(shouldShowDefaultWorkflowSaveAsNotice('default', baseline, baseline)).toBe(false);
    expect(shouldShowDefaultWorkflowSaveAsNotice('default', restored, baseline)).toBe(false);
    expect(shouldShowDefaultWorkflowSaveAsNotice('custom', changed, baseline)).toBe(false);
  });
});
