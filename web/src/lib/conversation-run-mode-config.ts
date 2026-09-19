import type { ConversationAutoConfigVm, ConversationDirectConfigVm, ConversationRunModeVm, WorkflowTemplate } from '@/types';

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

function normalizeOptionalRunModeId(value: string | null | undefined): string | undefined {
  return normalizeOptionalRunModeText(value)?.trim();
}

export function normalizeConversationAutoConfigForSubmit(
  config: ConversationAutoConfigVm | null | undefined,
): ConversationAutoConfigVm | undefined {
  if (!config) return undefined;
  const configOptions = normalizeConfigOptions(config.configOptions);
  const modelBoundOverrides = normalizeModelBoundOverrides(config.modelBoundOverrides);
  const bootstrapConfigOptions = normalizeConfigOptions(config.bootstrapConfigOptions);
  const bootstrapModelBoundOverrides = normalizeModelBoundOverrides(config.bootstrapModelBoundOverrides);
  const acceptanceConfigOptions = normalizeConfigOptions(config.acceptanceConfigOptions);
  const acceptanceModelBoundOverrides = normalizeModelBoundOverrides(config.acceptanceModelBoundOverrides);
  const availableAgents = config.availableAgents?.map((agent) => {
    const agentConfigOptions = normalizeConfigOptions(agent.configOptions);
    const agentModelBoundOverrides = normalizeModelBoundOverrides(agent.modelBoundOverrides);
    const {
      configOptions: _agentConfigOptions,
      autoAccept: _agentAutoAccept,
      modelBoundOverrides: _agentModelBoundOverrides,
      ...agentRest
    } = agent;
    return {
      ...agentRest,
      model: normalizeOptionalRunModeId(agent.model),
      permissionMode: normalizeOptionalRunModeId(agent.permissionMode),
      ...(agent.autoAccept ? { autoAccept: true } : {}),
      ...(agentConfigOptions ? { configOptions: agentConfigOptions } : {}),
      ...(agentModelBoundOverrides ? { modelBoundOverrides: agentModelBoundOverrides } : {}),
    };
  });
  const {
    configOptions: _configOptions,
    bootstrapConfigOptions: _bootstrapConfigOptions,
    acceptanceConfigOptions: _acceptanceConfigOptions,
    availableAgents: _availableAgents,
    bootstrapModelId: _bootstrapModelId,
    acceptanceModelId: _acceptanceModelId,
    modelId: _modelId,
    permissionMode: _permissionMode,
    autoAccept: _autoAccept,
    modelBoundOverrides: _modelBoundOverrides,
    bootstrapModelBoundOverrides: _bootstrapModelBoundOverrides,
    acceptanceModelBoundOverrides: _acceptanceModelBoundOverrides,
    ...rest
  } = config;
  const bootstrapModelId = normalizeOptionalRunModeId(config.bootstrapModelId);
  const acceptanceModelId = normalizeOptionalRunModeId(config.acceptanceModelId);
  const modelId = normalizeOptionalRunModeId(config.modelId);
  const permissionMode = normalizeOptionalRunModeId(config.permissionMode);
  return {
    ...rest,
    globalGoal: normalizeOptionalRunModeText(config.globalGoal),
    ...(bootstrapModelId ? { bootstrapModelId } : {}),
    ...(acceptanceModelId ? { acceptanceModelId } : {}),
    ...(modelId ? { modelId } : {}),
    ...(permissionMode ? { permissionMode } : {}),
    ...(config.autoAccept ? { autoAccept: true } : {}),
    ...(config.agentStrategy !== 'dynamic' && configOptions ? { configOptions } : {}),
    ...(modelBoundOverrides ? { modelBoundOverrides } : {}),
    ...(bootstrapConfigOptions ? { bootstrapConfigOptions } : {}),
    ...(bootstrapModelBoundOverrides ? { bootstrapModelBoundOverrides } : {}),
    ...(acceptanceConfigOptions ? { acceptanceConfigOptions } : {}),
    ...(acceptanceModelBoundOverrides ? { acceptanceModelBoundOverrides } : {}),
    ...(availableAgents ? { availableAgents } : {}),
  };
}

export function normalizeConversationDirectConfigForSubmit(
  config: ConversationDirectConfigVm | null | undefined,
): ConversationDirectConfigVm | undefined {
  if (!config?.agentType.trim()) return undefined;
  const configOptions = normalizeConfigOptions(config.configOptions);
  const modelBoundOverrides = normalizeModelBoundOverrides(config.modelBoundOverrides);
  return {
    agentType: config.agentType.trim(),
    modelId: normalizeOptionalRunModeId(config.modelId),
    permissionMode: normalizeOptionalRunModeId(config.permissionMode),
    ...(config.autoAccept ? { autoAccept: true } : {}),
    ...(configOptions ? { configOptions } : {}),
    ...(modelBoundOverrides ? { modelBoundOverrides } : {}),
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

function normalizeModelBoundOverrides(
  remembered: Record<string, Record<string, string>> | null | undefined,
): Record<string, Record<string, string>> | undefined {
  if (!remembered) return undefined;
  const next: Record<string, Record<string, string>> = {};
  for (const [modelId, overrides] of Object.entries(remembered)) {
    const id = modelId.trim();
    if (!id) continue;
    next[id] = normalizeConfigOptions(overrides) ?? {};
  }
  return Object.keys(next).length > 0 ? next : undefined;
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
    optionalEntryPreferences: patch.optionalEntryPreferences === undefined
      ? current.optionalEntryPreferences
      : patch.optionalEntryPreferences,
    directConfig: patch.directConfig === undefined ? current.directConfig : patch.directConfig,
    directPreferences: patch.directPreferences === undefined
      ? current.directPreferences
      : patch.directPreferences,
    autoConfig: patch.autoConfig === undefined ? current.autoConfig : patch.autoConfig,
  };
}

export function shouldShowOptionalEntryToggle(
  mode: ConversationRunModeVm['mode'],
  template: WorkflowTemplate | null | undefined,
): boolean {
  return mode === 'workflow' && Boolean(template?.optionalEntryStage);
}

export function includeOptionalEntryForSubmit(
  mode: ConversationRunModeVm,
  template: WorkflowTemplate | null | undefined,
): boolean | undefined {
  if (!shouldShowOptionalEntryToggle(mode.mode, template)) return undefined;
  return mode.optionalEntryPreferences?.[template!.id] ?? template!.optionalEntryStage!.defaultEnabled;
}

export function setOptionalEntryPreference(
  mode: ConversationRunModeVm,
  templateId: string,
  enabled: boolean,
): ConversationRunModeVm {
  return {
    ...mode,
    optionalEntryPreferences: {
      ...mode.optionalEntryPreferences,
      [templateId]: enabled,
    },
  };
}
