import type { WorkflowDsl, WorkflowTemplate } from '@/types';
import { DEFAULT_WORKFLOW_TEMPLATE_ID } from '@/lib/conversation-run-mode-config';

type Translate = (key: string, options?: Record<string, unknown>) => string;

export function workflowTemplateDisplayName(template: WorkflowTemplate, t: Translate): string {
  return template.id === DEFAULT_WORKFLOW_TEMPLATE_ID
    ? t('taskList.create.defaultWorkflow')
    : template.name;
}

export function hasWorkflowDraftChanges(
  workflow: WorkflowDsl | null | undefined,
  baseline: WorkflowDsl | null | undefined,
): boolean {
  return Boolean(workflow && baseline && JSON.stringify(workflow) !== JSON.stringify(baseline));
}

export function shouldShowDefaultWorkflowSaveAsNotice(
  templateId: string | null | undefined,
  workflow: WorkflowDsl | null | undefined,
  baseline: WorkflowDsl | null | undefined,
): boolean {
  return templateId === DEFAULT_WORKFLOW_TEMPLATE_ID
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
