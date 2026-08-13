import type { WorkflowDsl, WorkflowTemplate } from '@/types';
type Translate = (key: string, options?: Record<string, unknown>) => string;

export function workflowTemplateDisplayName(template: WorkflowTemplate, t: Translate): string {
  if (template.id === 'default') return t('taskList.create.defaultFullWorkflow');
  if (template.id === 'default-lightweight') return t('taskList.create.defaultLightweightWorkflow');
  return template.name;
}

export function hasWorkflowDraftChanges(
  workflow: WorkflowDsl | null | undefined,
  baseline: WorkflowDsl | null | undefined,
): boolean {
  return Boolean(workflow && baseline && JSON.stringify(workflow) !== JSON.stringify(baseline));
}

export function shouldShowDefaultWorkflowSaveAsNotice(
  template: WorkflowTemplate | null | undefined,
  workflow: WorkflowDsl | null | undefined,
  baseline: WorkflowDsl | null | undefined,
): boolean {
  return Boolean(template?.isBuiltIn)
    && hasWorkflowDraftChanges(workflow, baseline);
}

export function createBlankWorkflowDraft(): WorkflowDsl {
  return {
    version: '0.1',
    id: `workflow-${Date.now().toString(36)}`,
    entry: '',
    control: {},
    nodes: [],
    edges: [],
  };
}
