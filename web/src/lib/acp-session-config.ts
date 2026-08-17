import type {
  AcpModeVm,
  AcpSelectConfigOptionVm,
  AcpSessionConfigVm,
  AgentRegistryVm,
} from "@/types";

export type AcpSessionConfigCategory = string;

export type AcpSessionConfigOption = {
  id: string;
  name: string;
  description?: string | null;
  available?: boolean;
};

export type AcpSessionConfigGroup = {
  id: string;
  category: string;
  name: string | null;
  description: string | null;
  currentValue: string | null;
  overrideValue: string | null;
  overrideValueName: string | null;
  canSelectUnspecified: boolean;
  options: AcpSessionConfigOption[];
};

export type AcpProviderConfigCatalog = {
  observedAt: string;
  models: AcpModeVm[];
  modes: AcpModeVm[];
  configOptions: AcpSelectConfigOptionVm[];
};

export type AcpSessionConfigViewModel = {
  modelOverrideId: string | null;
  modelOverrideName: string | null;
  canSelectUnspecifiedModel: boolean;
  permissionModeOverrideId: string | null;
  permissionModeOverrideName: string | null;
  canSelectUnspecifiedPermissionMode: boolean;
  currentModelId: string | null;
  currentModelName: string | null;
  currentModeId: string | null;
  currentModeName: string | null;
  modeLabel: string | null;
  availableModels: AcpSessionConfigOption[];
  availablePermissionModes: AcpSessionConfigOption[];
  thoughtLevel: AcpSessionConfigGroup | null;
  signature: string;
};

export function createAcpSessionConfigViewModel(
  config: AcpSessionConfigVm | null | undefined,
  providerCatalog: AcpProviderConfigCatalog | null | undefined = null,
): AcpSessionConfigViewModel {
  const projectedCatalog = projectAcpSessionConfigCatalog(config, providerCatalog);
  const currentModelId = config?.currentModelId ?? null;
  const currentModelName = config?.currentModelName ?? null;
  const currentModeId = config?.currentModeId ?? null;
  const currentModeName = config?.currentModeName ?? null;
  const availableModels = normalizeAcpSessionConfigOptions(
    projectedCatalog.models,
    projectedCatalog.configOptions,
    "model",
  );
  const availablePermissionModes = normalizeAcpSessionConfigOptions(
    projectedCatalog.modes,
    projectedCatalog.configOptions,
    "mode",
  );
  const modelOverrideId = config?.modelOverrideId ?? null;
  const modelOverrideName = modelOverrideId
    ? availableModels.find((option) => option.id === modelOverrideId)?.name
      ?? (currentModelId === modelOverrideId ? currentModelName : null)
      ?? findAcpConfigOption(config?.models, config?.configOptions, "model", modelOverrideId).name
      ?? modelOverrideId
    : null;
  const permissionModeOverrideId = config?.permissionModeOverrideId ?? null;
  const thoughtLevel = normalizeAcpSelectConfigGroups(
    projectedCatalog.configOptions,
    config?.configOptionOverrides,
    config?.configOptions,
  ).find((group) => group.category === "thought_level") ?? null;
  const permissionModeOverrideName = permissionModeOverrideId
    ? availablePermissionModes.find((option) => option.id === permissionModeOverrideId)?.name
      ?? (currentModeId === permissionModeOverrideId ? currentModeName : null)
      ?? findAcpConfigOption(config?.modes, config?.configOptions, "mode", permissionModeOverrideId).name
      ?? permissionModeOverrideId
    : null;
  const projectedAvailableModels = withUnavailableCurrentOption(
    availableModels,
    modelOverrideId,
    modelOverrideName,
  );
  const projectedAvailablePermissionModes = withUnavailableCurrentOption(
    availablePermissionModes,
    permissionModeOverrideId,
    permissionModeOverrideName,
  );
  const resolvedCurrentModelName = currentModelName ?? (
    currentModelId ? null : singleOptionName(projectedAvailableModels)
  );
  const resolvedCurrentModeName = currentModeName ?? (
    currentModeId ? null : singleOptionName(projectedAvailablePermissionModes)
  );
  const resolvedModeLabel = resolvedCurrentModeName ?? currentModeId;
  const viewModel = {
    modelOverrideId,
    modelOverrideName,
    canSelectUnspecifiedModel: modelOverrideId === null,
    permissionModeOverrideId,
    permissionModeOverrideName,
    canSelectUnspecifiedPermissionMode: permissionModeOverrideId === null,
    currentModelId,
    currentModelName: resolvedCurrentModelName,
    currentModeId,
    currentModeName: resolvedCurrentModeName,
    modeLabel: resolvedModeLabel,
    availableModels: projectedAvailableModels,
    availablePermissionModes: projectedAvailablePermissionModes,
    thoughtLevel,
  };

  return {
    ...viewModel,
    signature: createAcpSessionConfigSignature(viewModel),
  };
}

export function normalizeAcpSelectConfigGroups(
  configOptions: unknown,
  overrides: Record<string, string> | null | undefined = undefined,
  fallbackConfigOptions: unknown = undefined,
): AcpSessionConfigGroup[] {
  return (arrayValue(configOptions) ?? []).flatMap((raw) => {
    const option = rawObject(raw);
    const id = stringValue(option?.id)?.trim();
    if (!id || stringValue(option?.type) !== "select") return [];
    const category = stringValue(option?.category)?.trim() || id;
    const options = normalizeConfigOptionList(arrayValue(option?.options), category);
    if (options.length === 0) return [];
    const overrideValue = overrides?.[id]?.trim() || null;
    const overrideValueName = overrideValue
      ? options.find((option) => option.id === overrideValue)?.name
        ?? configOptionValueName(fallbackConfigOptions, id, overrideValue)
        ?? overrideValue
      : null;
    return [{
      id,
      category,
      name: stringValue(option?.name)?.trim() || null,
      description: stringValue(option?.description)?.trim() || null,
      currentValue: stringValue(option?.currentValue)?.trim() || null,
      overrideValue,
      overrideValueName,
      canSelectUnspecified: overrideValue === null,
      options: withUnavailableCurrentOption(options, overrideValue, overrideValueName),
    }];
  });
}

function configOptionValueName(
  configOptions: unknown,
  optionId: string,
  value: string,
) {
  const option = arrayValue(configOptions)
    ?.map(rawObject)
    .find((candidate) => stringValue(candidate?.id) === optionId);
  return normalizeConfigOptionList(arrayValue(option?.options), optionId)
    .find((candidate) => candidate.id === value)
    ?.name ?? null;
}

export function acpProviderConfigCatalog(
  registry: AgentRegistryVm | null | undefined,
  provider: string | null | undefined,
): AcpProviderConfigCatalog | null {
  if (!provider) return null;
  const agent = registry?.agents.find((candidate) => candidate.agentType === provider);
  if (!agent?.diagnostic?.available || !agent.diagnostic.checkedAt) return null;
  return {
    observedAt: agent.diagnostic.checkedAt,
    models: agent.supportedModels ?? [],
    modes: agent.supportedModes ?? [],
    configOptions: agent.configOptions ?? [],
  };
}

export function isAcpCatalogObservationNewer(
  candidate: string | null | undefined,
  current: string | null | undefined,
) {
  const candidateValue = catalogObservationValue(candidate);
  if (!candidateValue) return false;
  const currentValue = catalogObservationValue(current);
  if (!currentValue) return true;
  if (candidateValue.raw === currentValue.raw) return false;
  if (candidateValue.epoch !== null && currentValue.epoch !== null) {
    return candidateValue.epoch > currentValue.epoch;
  }
  return candidateValue.raw > currentValue.raw;
}

function projectAcpSessionConfigCatalog(
  config: AcpSessionConfigVm | null | undefined,
  providerCatalog: AcpProviderConfigCatalog | null | undefined,
) {
  if (!providerCatalog || !isAcpCatalogObservationNewer(
    providerCatalog.observedAt,
    config?.catalogObservedAt,
  )) {
    return {
      models: config?.models,
      modes: config?.modes,
      configOptions: config?.configOptions,
    };
  }
  return {
    models: {
      availableModels: providerCatalog.models.map((model) => ({
        modelId: model.id,
        name: model.name,
        description: model.description,
      })),
    },
    modes: {
      availableModes: providerCatalog.modes.map((mode) => ({
        id: mode.id,
        name: mode.name,
        description: mode.description,
      })),
    },
    configOptions: mergeProviderCatalogCurrentValues(
      providerCatalog.configOptions,
      config?.configOptions,
    ),
  };
}

function mergeProviderCatalogCurrentValues(
  providerOptions: AcpSelectConfigOptionVm[],
  sessionOptions: unknown,
) {
  const sessionOptionList = arrayValue(sessionOptions)?.map(rawObject) ?? [];
  return providerOptions.map((option) => {
    const sessionOption = sessionOptionList.find((candidate) =>
      stringValue(candidate?.id) === option.id
      || (
        option.category
        && stringValue(candidate?.category) === option.category
      ));
    return {
      ...option,
      type: "select",
      currentValue: stringValue(sessionOption?.currentValue)?.trim() || undefined,
    };
  });
}

function catalogObservationValue(value: string | null | undefined) {
  const raw = value?.trim();
  if (!raw) return null;
  const epochMatch = /^(\d+)Z?$/.exec(raw);
  if (epochMatch) return { raw, epoch: Number(epochMatch[1]) };
  const parsed = Date.parse(raw);
  return { raw, epoch: Number.isFinite(parsed) ? Math.floor(parsed / 1000) : null };
}

export function findAcpConfigOption(
  groupedOptions: unknown,
  configOptions: unknown,
  category: AcpSessionConfigCategory,
  id: string,
): AcpSessionConfigOption {
  const configMatch = configOptionValues(configOptions, category).find(
    (option) => option.id === id,
  );
  if (configMatch) return configMatch;

  const groupedMatch = groupedConfigOptions(groupedOptions, category).find(
    (option) => option.id === id,
  );
  return groupedMatch ?? { id, name: id };
}

export function normalizeAcpSessionConfigOptions(
  groupedOptions: unknown,
  configOptions: unknown,
  category: AcpSessionConfigCategory,
): AcpSessionConfigOption[] {
  const configured = configOptionValues(configOptions, category);
  if (configured.length > 0) return configured;
  return groupedConfigOptions(groupedOptions, category);
}

function createAcpSessionConfigSignature(
  viewModel: Omit<AcpSessionConfigViewModel, "signature">,
) {
  return JSON.stringify({
    modelOverrideId: viewModel.modelOverrideId,
    modelOverrideName: viewModel.modelOverrideName,
    canSelectUnspecifiedModel: viewModel.canSelectUnspecifiedModel,
    permissionModeOverrideId: viewModel.permissionModeOverrideId,
    permissionModeOverrideName: viewModel.permissionModeOverrideName,
    canSelectUnspecifiedPermissionMode: viewModel.canSelectUnspecifiedPermissionMode,
    currentModelId: viewModel.currentModelId,
    currentModelName: viewModel.currentModelName,
    currentModeId: viewModel.currentModeId,
    currentModeName: viewModel.currentModeName,
    models: viewModel.availableModels.map(signatureOption),
    modes: viewModel.availablePermissionModes.map(signatureOption),
    thoughtLevel: viewModel.thoughtLevel ? {
      id: viewModel.thoughtLevel.id,
      currentValue: viewModel.thoughtLevel.currentValue,
      overrideValue: viewModel.thoughtLevel.overrideValue,
      overrideValueName: viewModel.thoughtLevel.overrideValueName,
      canSelectUnspecified: viewModel.thoughtLevel.canSelectUnspecified,
      options: viewModel.thoughtLevel.options.map(signatureOption),
    } : null,
  });
}

function signatureOption(option: AcpSessionConfigOption) {
  return [option.id, option.name, option.description ?? null, option.available ?? true];
}

function withUnavailableCurrentOption(
  options: AcpSessionConfigOption[],
  value: string | null,
  name: string | null,
) {
  if (!value || options.some((option) => option.id === value)) return options;
  return [
    { id: value, name: name ?? value, description: null, available: false },
    ...options,
  ];
}

function singleOptionName(options: AcpSessionConfigOption[]) {
  return options.length === 1 ? options[0]?.name ?? null : null;
}

function groupedConfigOptions(
  groupedOptions: unknown,
  category: AcpSessionConfigCategory,
) {
  const grouped = rawObject(groupedOptions);
  if (category !== "model" && category !== "mode") return [];
  const preferredKey = category === "model" ? "availableModels" : "availableModes";
  const fallbackKey = category === "model" ? "availableModes" : "availableModels";
  const list = arrayValue(grouped?.[preferredKey]) ?? arrayValue(grouped?.[fallbackKey]);
  return normalizeConfigOptionList(list, category);
}

function configOptionValues(
  configOptions: unknown,
  category: AcpSessionConfigCategory,
) {
  const configOption = arrayValue(configOptions)
    ?.map(rawObject)
    .find(
      (option) =>
        stringValue(option?.id) === category ||
        stringValue(option?.category) === category,
    );
  return normalizeConfigOptionList(arrayValue(configOption?.options), category);
}

function normalizeConfigOptionList(
  list: unknown[] | null | undefined,
  category: AcpSessionConfigCategory,
) {
  if (!Array.isArray(list)) return [];
  const ids = new Set<string>();
  const options: AcpSessionConfigOption[] = [];
  for (const item of list) {
    const option = rawObject(item);
    if (!option) continue;
    const id =
      (category === "model" ? stringValue(option.modelId) : null) ??
      stringValue(option.id) ??
      stringValue(option.value);
    if (!id || ids.has(id)) continue;
    ids.add(id);
    const name = stringValue(option.name)?.trim() || id;
    const description = stringValue(option.description)?.trim() || null;
    options.push({ id, name, description });
  }
  return options;
}

function rawObject(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function arrayValue(value: unknown): unknown[] | null {
  return Array.isArray(value) ? value : null;
}

function stringValue(value: unknown) {
  return typeof value === "string" ? value : null;
}
