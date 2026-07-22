import type { ConversationAutoConfigVm, ConversationRunModeVm } from '@/types';

export const DEFAULT_CONVERSATION_RUN_MODE: ConversationRunModeVm = { mode: 'auto' };
export const DEFAULT_WORKFLOW_TEMPLATE_ID = 'default';

export function conversationRunModeOrDefault(
  mode: ConversationRunModeVm | null | undefined,
): ConversationRunModeVm {
  return mode ?? DEFAULT_CONVERSATION_RUN_MODE;
}

export function optionalRunModeText(value: string | null | undefined): string | undefined {
  return typeof value === 'string' && value.length > 0 ? value : undefined;
}

export function normalizeOptionalRunModeText(value: string | null | undefined): string | undefined {
  if (typeof value !== 'string') return undefined;
  return value.trim().length > 0 ? value : undefined;
}

export function normalizeConversationAutoConfigForSubmit(
  config: ConversationAutoConfigVm | null | undefined,
): ConversationAutoConfigVm | undefined {
  if (!config) return undefined;
  return {
    ...config,
    globalGoal: normalizeOptionalRunModeText(config.globalGoal),
  };
}

export function mergeConversationRunMode(
  current: ConversationRunModeVm,
  patch: ConversationRunModeVm,
): ConversationRunModeVm {
  return {
    mode: patch.mode,
    workflowTemplateId: patch.workflowTemplateId === undefined
      ? current.workflowTemplateId
      : patch.workflowTemplateId,
    includeInterview: patch.includeInterview === undefined
      ? current.includeInterview
      : patch.includeInterview,
    autoConfig: patch.autoConfig === undefined ? current.autoConfig : patch.autoConfig,
  };
}

export function isDefaultWorkflowTemplate(templateId: string | null | undefined): boolean {
  return templateId === DEFAULT_WORKFLOW_TEMPLATE_ID;
}

export function shouldShowInterviewToggle(
  mode: ConversationRunModeVm['mode'],
  templateId: string | null | undefined,
): boolean {
  return mode === 'workflow' && isDefaultWorkflowTemplate(templateId);
}

export function includeInterviewForSubmit(
  mode: ConversationRunModeVm,
  templateId: string | null | undefined,
): boolean | undefined {
  if (!shouldShowInterviewToggle(mode.mode, templateId)) return undefined;
  return mode.includeInterview ?? true;
}
