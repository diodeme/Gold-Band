import { describe, expect, it } from 'vitest';

import { hasWorkflowBindingDraftChanges, hasWorkflowDraftChanges, restoreBuiltInWorkflowDefinition, shouldShowDefaultWorkflowSaveAsNotice, workflowTemplateDisplayName } from '@/lib/workflow-template';

const defaultTemplate = {
  id: 'default',
  name: '默认完整工作流',
  isBuiltIn: true,
  workflow: { version: '0.1', id: 'default-workflow', entry: '', control: {}, nodes: [], edges: [] },
  modelBindings: { definitionRevision: '', bindingRevision: 0, bindings: [] },
  createdAt: '2026-08-07T00:00:00Z',
  updatedAt: '2026-08-07T00:00:00Z',
};

describe('workflow template display names', () => {
  it('localizes the built-in template from its stable ID instead of its persisted name', () => {
    const t = (key: string) => key === 'taskList.create.defaultFullWorkflow' ? 'Default full workflow' : key;
    expect(workflowTemplateDisplayName(defaultTemplate, t)).toBe('Default full workflow');
    expect(workflowTemplateDisplayName({ ...defaultTemplate, id: 'default-lightweight' }, (key) => (
      key === 'taskList.create.defaultLightweightWorkflow' ? 'Default lightweight workflow' : key
    ))).toBe('Default lightweight workflow');
  });

  it('preserves user-defined template names', () => {
    const t = (key: string) => key;
    expect(workflowTemplateDisplayName({ ...defaultTemplate, id: 'custom', name: 'Release checklist', isBuiltIn: false }, t)).toBe('Release checklist');
  });

  it('shows the save-as notice only after the built-in default workflow changes', () => {
    const baseline = { ...defaultTemplate.workflow, entry: 'plan' };
    const changed = { ...baseline, entry: 'dev' };
    const restored = { ...baseline, model: undefined, config_options: undefined };

    expect(hasWorkflowDraftChanges(changed, baseline)).toBe(true);
    expect(shouldShowDefaultWorkflowSaveAsNotice(defaultTemplate, changed, baseline)).toBe(true);
    expect(shouldShowDefaultWorkflowSaveAsNotice(defaultTemplate, baseline, baseline)).toBe(false);
    expect(shouldShowDefaultWorkflowSaveAsNotice(defaultTemplate, restored, baseline)).toBe(false);
    expect(shouldShowDefaultWorkflowSaveAsNotice({ ...defaultTemplate, id: 'custom', isBuiltIn: false }, changed, baseline)).toBe(false);
  });

  it('classifies binding changes independently from definition changes', () => {
    const baseline = { definitionRevision: 'a', bindingRevision: 1, bindings: [{ executionSlotId: 'slot-a', agentId: 'agent-a' }] };
    const changed = { ...baseline, bindingRevision: 2, bindings: [{ executionSlotId: 'slot-a', agentId: 'agent-b' }] };
    expect(hasWorkflowBindingDraftChanges({ ...baseline, bindingRevision: 99 }, baseline)).toBe(false);
    expect(hasWorkflowBindingDraftChanges(changed, baseline)).toBe(true);
  });

  it('restores a built-in definition while preserving only bindings owned by its slots', () => {
    const workflow = {
      ...defaultTemplate.workflow,
      entry: 'worker',
      nodes: [{ id: 'worker', type: 'worker' as const, executionSlotId: 'slot-a', profile: 'developer' }],
    };
    const restored = restoreBuiltInWorkflowDefinition(workflow, {
      definitionRevision: 'draft',
      bindingRevision: 3,
      bindings: [
        { executionSlotId: 'slot-a', agentId: 'agent-a' },
        { executionSlotId: 'temporary-slot', agentId: 'agent-b' },
      ],
    });
    expect(restored.workflow).toEqual(workflow);
    expect(restored.workflow).not.toBe(workflow);
    expect(restored.modelBindings.bindings).toEqual([{ executionSlotId: 'slot-a', agentId: 'agent-a' }]);
  });
});
