import { describe, expect, it } from 'vitest';
import { validateWorkflowForSave } from '@/components/WorkflowEditor';
import { readyWorkflowProfileCatalog } from '@/lib/workflow-profile-catalog';
import { retainAcpModelBoundOverrides, projectAuthoringConfigOverrides } from '@/lib/acp-composite-config';
import { updateAcpConfigOptionOverride } from '@/components/acp/AcpModelThoughtSelects';
import type { ManagedAgentVm, WorkflowDsl } from '@/types';
import { reconcileWorkflowModelBindings } from '@/lib/workflow-model-bindings';

const options = [
  { id: 'model', category: 'model', currentValue: 'observed', options: [{ value: 'observed' }, { value: 'unobserved' }] },
  { id: 'effort', category: 'thought_level', options: [{ value: 'xhigh', name: 'Extra High' }] },
  { id: 'fast', category: 'model_config', options: [{ value: 'false', name: 'Off' }] },
];
const agent = {
  agentType: 'cursor', displayName: 'Cursor', diagnostic: { available: true, status: 'healthy' },
  supportedModels: [{ id: 'observed' }, { id: 'unobserved' }],
  configOptions: options, modelBoundCatalogs: { observed: options.slice(1) },
} as ManagedAgentVm;
const workflow: WorkflowDsl = {
  version: '0.1', id: 'config-discovery', entry: 'grill', control: {},
  nodes: [{ type: 'worker', id: 'grill', executionSlotId: 'slot', profile: 'developer', goal: 'Implement' }],
  edges: [{ from: 'grill', to: '$end', on: 'success' }],
};
function validate(modelId: string, configOptions: Record<string, string>) {
  return validateWorkflowForSave(workflow, readyWorkflowProfileCatalog([{ id: 'developer', name: 'Developer' }]),
    [agent], (key, params) => `${key}:${params?.option ?? ''}`, null, null, null, true,
    { definitionRevision: '', bindingRevision: 0, bindings: [{ executionSlotId: 'slot', agentId: 'cursor', modelId, configOptions }] });
}

describe('workflow model configuration discovery boundaries', () => {
  it('allows unobserved model settings without treating another model catalog as authoritative', () => {
    const overrides = { context: '500k', reasoning_effort: 'xhigh', fast: 'false' };
    expect(validate('unobserved', overrides).issues).toEqual([]);
    expect(retainAcpModelBoundOverrides(overrides, options, 'unobserved', agent.modelBoundCatalogs)).toEqual(overrides);
  });

  it('validates an observed model using the same thought remapping as the selector', () => {
    expect(validate('observed', { reasoning_effort: 'xhigh', fast: 'false' }).issues).toEqual([]);
  });

  it('keeps rejecting a listed but invalid value for an observed model', () => {
    expect(validate('observed', { effort: 'invalid' }).valid).toBe(false);
  });

  it('lets an explicit unspecified selection clear the displayed thought without deleting unrelated unknown settings', () => {
    const values = projectAuthoringConfigOverrides({ reasoning_effort: 'xhigh', context: '500k' }, options, agent.modelBoundCatalogs, 'unobserved');
    expect(updateAcpConfigOptionOverride(values, 'effort', null)).toEqual({ context: '500k' });
  });

  it('saves remapped fields and model memory together without mutating the input', () => {
    const old = { reasoning_effort: 'xhigh', context: '500k', fast: 'false' };
    const bindings = { definitionRevision: '', bindingRevision: 1, bindings: [{
      executionSlotId: 'slot', agentId: 'cursor', modelId: 'observed', configOptions: old,
      modelBoundOverrides: { observed: old, unobserved: old },
    }] };
    const next = reconcileWorkflowModelBindings(bindings, [agent]);
    expect(next.bindings[0].configOptions).toEqual({ effort: 'xhigh', fast: 'false' });
    expect(next.bindings[0].modelBoundOverrides?.observed).toEqual(next.bindings[0].configOptions);
    expect(next.bindings[0].modelBoundOverrides?.unobserved).toEqual(old);
    expect(bindings.bindings[0].configOptions).toEqual(old);
    expect(reconcileWorkflowModelBindings(next, [agent])).toEqual(next);
  });
});
