import type { ConversationAutoConfigVm, ConversationDirectConfigVm, ConversationRunModeVm } from '@/types';

export const DEFAULT_CONVERSATION_RUN_MODE: ConversationRunModeVm = { mode: 'direct' };
export const DEFAULT_WORKFLOW_TEMPLATE_ID = 'default';
export const CONVERSATION_RUN_MODE_ORDER: ConversationRunModeVm['mode'][] = ['direct', 'workflow', 'auto'];
export type ConversationRunModesByWorkspace = Record<string, ConversationRunModeVm>;

export function canOpenRunModeManagement(mode: ConversationRunModeVm['mode']): boolean {
  return mode !== 'direct';
}

export function conversationRunModeOrDefault(
  mode: ConversationRunModeVm | null | undefined,
): ConversationRunModeVm {
  return mode ?? DEFAULT_CONVERSATION_RUN_MODE;
}

export function conversationRunModeForWorkspace(
  modes: ConversationRunModesByWorkspace,
  projectId: string,
): ConversationRunModeVm {
  return conversationRunModeOrDefault(modes[projectId]);
}

export function setConversationRunModeForWorkspace(
  modes: ConversationRunModesByWorkspace,
  projectId: string,
  mode: ConversationRunModeVm,
): ConversationRunModesByWorkspace {
  return {
    ...modes,
    [projectId]: mode,
  };
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
  const configOptions = normalizeConfigOptions(config.configOptions);
  const bootstrapConfigOptions = normalizeConfigOptions(config.bootstrapConfigOptions);
  const acceptanceConfigOptions = normalizeConfigOptions(config.acceptanceConfigOptions);
  const availableAgents = config.availableAgents?.map((agent) => {
    const agentConfigOptions = normalizeConfigOptions(agent.configOptions);
    const { configOptions: _agentConfigOptions, ...agentRest } = agent;
    return { ...agentRest, ...(agentConfigOptions ? { configOptions: agentConfigOptions } : {}) };
  });
  const {
    configOptions: _configOptions,
    bootstrapConfigOptions: _bootstrapConfigOptions,
    acceptanceConfigOptions: _acceptanceConfigOptions,
    availableAgents: _availableAgents,
    ...rest
  } = config;
  return {
    ...rest,
    globalGoal: normalizeOptionalRunModeText(config.globalGoal),
    ...(configOptions ? { configOptions } : {}),
    ...(bootstrapConfigOptions ? { bootstrapConfigOptions } : {}),
    ...(acceptanceConfigOptions ? { acceptanceConfigOptions } : {}),
    ...(availableAgents ? { availableAgents } : {}),
  };
}

export function normalizeConversationDirectConfigForSubmit(
  config: ConversationDirectConfigVm | null | undefined,
): ConversationDirectConfigVm | undefined {
  if (!config?.agentType.trim()) return undefined;
  const configOptions = normalizeConfigOptions(config.configOptions);
  return {
    agentType: config.agentType.trim(),
    modelId: normalizeOptionalRunModeText(config.modelId),
    permissionMode: normalizeOptionalRunModeText(config.permissionMode),
    ...(configOptions ? { configOptions } : {}),
  };
}

function normalizeConfigOptions(options: Record<string, string> | null | undefined) {
  if (!options) return undefined;
  const normalized = Object.fromEntries(
    Object.entries(options)
      .map(([key, value]) => [key.trim(), value.trim()] as const)
      .filter(([key, value]) => key.length > 0 && value.length > 0),
  );
  return Object.keys(normalized).length > 0 ? normalized : undefined;
}

export function directConfigForAgent(
  mode: ConversationRunModeVm,
  agentType: string,
): ConversationDirectConfigVm {
  return mode.directPreferences?.[agentType]
    ?? (mode.directConfig?.agentType === agentType ? mode.directConfig : undefined)
    ?? { agentType };
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
    directConfig: patch.directConfig === undefined ? current.directConfig : patch.directConfig,
    directPreferences: patch.directPreferences === undefined
      ? current.directPreferences
      : patch.directPreferences,
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
