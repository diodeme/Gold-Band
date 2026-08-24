import type { WorkflowModelBindings } from '@/types';

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
