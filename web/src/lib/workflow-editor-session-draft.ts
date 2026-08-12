import type { WorkflowEditorSessionDraft } from '@/components/WorkflowEditor';
import type { WorkflowDsl } from '@/types';

export function workflowEditorSessionDraftIsDirty(
  baselineWorkflow: WorkflowDsl | null,
  draft: WorkflowEditorSessionDraft,
) {
  return JSON.stringify(draft.workflow) !== (baselineWorkflow ? JSON.stringify(baselineWorkflow) : '')
    || draft.jsonDraft !== JSON.stringify(draft.workflow, null, 2);
}
