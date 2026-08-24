import type { WorkflowEditorSessionDraft } from '@/components/WorkflowEditor';
import type { WorkflowDsl, WorkflowModelBindings } from '@/types';

export function workflowEditorSessionDraftIsDirty(
  baselineWorkflow: WorkflowDsl | null,
  baselineModelBindings: WorkflowModelBindings | null,
  draft: WorkflowEditorSessionDraft,
) {
  return JSON.stringify(draft.workflow) !== (baselineWorkflow ? JSON.stringify(baselineWorkflow) : '')
    || JSON.stringify(draft.modelBindings.bindings) !== JSON.stringify(baselineModelBindings?.bindings ?? [])
    || draft.jsonDraft !== JSON.stringify(draft.workflow, null, 2);
}
