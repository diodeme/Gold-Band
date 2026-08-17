import { describe, expect, it } from 'vitest';

import { applyWorkerBindingSync, planWorkerBindingSync } from '@/components/WorkflowEditor';
import type { WorkflowDsl, WorkflowModelBindings } from '@/types';

const workflow: WorkflowDsl = {
  version: '0.1',
  id: 'sync-test',
  entry: 'a',
  control: {},
  nodes: [
    { id: 'a', type: 'worker', executionSlotId: 'slot-a', profile: 'developer' },
    { id: 'b', type: 'worker', executionSlotId: 'slot-b', profile: 'developer' },
    { id: 'c', type: 'worker', executionSlotId: 'slot-c', profile: 'developer' },
    { id: 'dynamic', type: 'ai-dynamic', profile: 'developer', agentStrategy: { type: 'fixed', agent: { provider: 'agent-a' } }, control: { maxDynamicNodes: 1, maxFanout: 1, maxDepth: 1, maxParallel: 1, maxGroupDepth: 1, maxWorkflowInvocations: 1, allowNestedDynamic: false }, allowedWorkflows: [] },
  ],
  edges: [],
};

const bindings: WorkflowModelBindings = {
  definitionRevision: 'definition',
  bindingRevision: 2,
  bindings: [
    { executionSlotId: 'slot-a', agentId: 'agent-a', modelId: 'model-a', permissionModeId: 'ask', configOptions: { thought: 'high' } },
    { executionSlotId: 'slot-c', agentId: 'agent-c', modelId: 'model-c' },
  ],
};

describe('worker model binding sync', () => {
  it('fills unconfigured workers and skips configured workers by default', () => {
    expect(planWorkerBindingSync(workflow, bindings, 'slot-a', false)).toEqual({
      fillCount: 1,
      overwriteCount: 0,
      skipCount: 1,
      targetSlotIds: ['slot-b'],
    });
    const result = applyWorkerBindingSync(workflow, bindings, 'slot-a', false);
    expect(result.bindings.find((binding) => binding.executionSlotId === 'slot-b')).toEqual({
      ...bindings.bindings[0],
      executionSlotId: 'slot-b',
    });
    expect(result.bindings.find((binding) => binding.executionSlotId === 'slot-c')?.agentId).toBe('agent-c');
  });

  it('overwrites configured workers in one immutable snapshot update', () => {
    expect(planWorkerBindingSync(workflow, bindings, 'slot-a', true)).toMatchObject({ fillCount: 1, overwriteCount: 1, skipCount: 0 });
    const result = applyWorkerBindingSync(workflow, bindings, 'slot-a', true);
    expect(result).not.toBe(bindings);
    expect(result.bindings.filter((binding) => binding.agentId === 'agent-a')).toHaveLength(3);
    expect(result.bindings.find((binding) => binding.executionSlotId === 'slot-c')?.configOptions).toEqual({ thought: 'high' });
  });
});
