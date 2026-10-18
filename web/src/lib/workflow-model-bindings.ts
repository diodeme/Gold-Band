import type { ManagedAgentVm, WorkflowModelBindings } from '@/types';
import { normalizeAuthoringConfigOverrides } from './acp-composite-config';

export function reconcileWorkflowModelBindings(value: WorkflowModelBindings, agents: readonly ManagedAgentVm[]): WorkflowModelBindings {
  const byId = new Map(agents.map((agent) => [agent.agentType, agent]));
  return { ...value, bindings: value.bindings.map((binding) => {
    const agent = byId.get(binding.agentId);
    if (!agent) return binding;
    const configOptions = normalizeAuthoringConfigOverrides(binding.configOptions, agent.configOptions, agent.modelBoundCatalogs, binding.modelId);
    return { ...binding, configOptions, modelBoundOverrides: binding.modelId
      ? { ...binding.modelBoundOverrides, [binding.modelId]: { ...configOptions } }
      : binding.modelBoundOverrides };
  }) };
}

export function normalizeWorkflowModelBindings(
  value: WorkflowModelBindings | null | undefined,
): WorkflowModelBindings {
  const candidate = value as Partial<WorkflowModelBindings> | null | undefined;
  if (
    candidate
    && typeof candidate.definitionRevision === 'string'
    && typeof candidate.bindingRevision === 'number'
    && Array.isArray(candidate.bindings)
  ) {
    return candidate as WorkflowModelBindings;
  }
  return {
    definitionRevision: candidate?.definitionRevision ?? '',
    bindingRevision: candidate?.bindingRevision ?? 0,
    bindings: Array.isArray(candidate?.bindings) ? candidate.bindings : [],
  };
}
