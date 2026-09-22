import type {
  AcpModeVm,
  AcpSelectConfigOptionVm,
  AcpSessionConfigVm,
  AgentRegistryVm,
} from "@/types";
import {
  ACP_MODEL_CONFIG_CATEGORY,
  ACP_THOUGHT_LEVEL_CATEGORY,
  isAcpModelBoundConfigCategory,
  remapAcpThoughtLevelOverride,
} from "@/lib/acp-composite-config";

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
  modelBoundCatalogs?: Record<string, AcpSelectConfigOptionVm[]> | null;
};

export type AcpSessionConfigViewModel = {
  modelOverrideId: string | null;
  modelOverrideName: string | null;
  canSelectUnspecifiedModel: boolean;
  permissionModeOverrideId: string | null;
  permissionModeOverrideName: string | null;
  canSelectUnspecifiedPermissionMode: boolean;
  autoAccept: boolean;
  currentModelId: string | null;
  currentModelName: string | null;
  currentModeId: string | null;
  currentModeName: string | null;
  modeLabel: string | null;
  availableModels: AcpSessionConfigOption[];
  availablePermissionModes: AcpSessionConfigOption[];
  thoughtLevel: AcpSessionConfigGroup | null;
  modelBoundOptions: AcpSessionConfigGroup[];
  signature: string;
};

export function mergeLiveAcpSessionConfig(
  current: AcpSessionConfigVm | null | undefined,
  incoming: AcpSessionConfigVm | null | undefined,
  preserveUserOverrides: boolean,
): AcpSessionConfigVm | null {
  if (!incoming) return current ?? null;
  if (!preserveUserOverrides || !current) return incoming;
  return {
    ...incoming,
    modelOverrideId: current.modelOverrideId,
    permissionModeOverrideId: current.permissionModeOverrideId,
    autoAccept: current.autoAccept,
    configOptionOverrides: current.configOptionOverrides,
    modelBoundOverrides: current.modelBoundOverrides,
    currentModelId: current.currentModelId,
    currentModelName: current.currentModelName,
    currentModeId: current.currentModeId,
    currentModeName: current.currentModeName,
  };
}

export function createAcpSessionConfigViewModel(
  config: AcpSessionConfigVm | null | undefined,
  providerCatalog: AcpProviderConfigCatalog | null | undefined = null,
  authoringModelBoundCatalogs: Record<string, AcpSelectConfigOptionVm[]> | null | undefined = undefined,
): AcpSessionConfigViewModel {
  const projectedCatalog = projectAcpSessionConfigCatalog(
    config,
    providerCatalog,
    authoringModelBoundCatalogs ?? providerCatalog?.modelBoundCatalogs,
  );
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
  const remappedOverrides = remapAcpThoughtLevelOverride(
    config?.configOptionOverrides,
    selectConfigOptionsFromUnknown(projectedCatalog.configOptions),
    [
      ...(selectConfigOptionsFromUnknown(config?.configOptions) ?? []),
      ...Object.values(sessionModelBoundCatalogs(config?.modelBoundCatalogs) ?? {}).flat(),
      ...Object.values(authoringModelBoundCatalogs ?? {}).flat(),
    ],
  );
  const catalogGroups = normalizeAcpSelectConfigGroups(
    projectedCatalog.configOptions,
    remappedOverrides,
    config?.configOptions,
  );
  const modelBoundOptions = [
    ...catalogGroups.filter((group) => group.category === ACP_THOUGHT_LEVEL_CATEGORY),
    ...catalogGroups.filter((group) => group.category === ACP_MODEL_CONFIG_CATEGORY),
  ];
  const thoughtLevel = modelBoundOptions.find((group) => group.category === ACP_THOUGHT_LEVEL_CATEGORY) ?? null;
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
    autoAccept: Boolean(config?.autoAccept),
    currentModelId,
    currentModelName: resolvedCurrentModelName,
    currentModeId,
    currentModeName: resolvedCurrentModeName,
    modeLabel: resolvedModeLabel,
    availableModels: projectedAvailableModels,
    availablePermissionModes: projectedAvailablePermissionModes,
    thoughtLevel,
    modelBoundOptions,
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

export function acpAuthoringModelBoundCatalogs(
  registry: AgentRegistryVm | null | undefined,
  provider: string | null | undefined,
): Record<string, AcpSelectConfigOptionVm[]> | undefined {
  const agentType = provider?.trim();
  if (!agentType) return undefined;
  return registry?.agents.find((agent) => agent.agentType === agentType)?.modelBoundCatalogs
    ?? undefined;
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
    modelBoundCatalogs: agent.modelBoundCatalogs ?? null,
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
  authoringModelBoundCatalogs: Record<string, AcpSelectConfigOptionVm[]> | null | undefined,
) {
  if (!providerCatalog || !isAcpCatalogObservationNewer(
    providerCatalog.observedAt,
    config?.catalogObservedAt,
  )) {
    return {
      models: config?.models,
      modes: config?.modes,
      configOptions: projectBoundConfigOptionsForSelectedModel(
        config,
        authoringModelBoundCatalogs,
      ),
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
    configOptions: mergeDoctorAndSessionConfigOptions(
      providerCatalog.configOptions,
      projectBoundConfigOptionsForSelectedModel(config, authoringModelBoundCatalogs),
    ),
  };
}

function projectBoundConfigOptionsForSelectedModel(
  config: AcpSessionConfigVm | null | undefined,
  authoringModelBoundCatalogs: Record<string, AcpSelectConfigOptionVm[]> | null | undefined,
) {
  const options = config?.configOptions;
  const selected = (config?.modelOverrideId ?? config?.currentModelId)?.trim()
    || catalogModelCurrentValue(options);
  const liveOwner = catalogModelCurrentValue(options);
  if (selected && selected === liveOwner && hasModelBoundConfigRows(options)) {
    return options;
  }
  const bound = lookupModelBoundCatalog(
    selected,
    config?.modelBoundCatalogs,
    authoringModelBoundCatalogs,
  );
  if (bound !== undefined) {
    return spliceBoundOptions(options, bound);
  }
  return options;
}

function sessionModelBoundCatalogs(
  catalogs: Record<string, unknown[]> | null | undefined,
) {
  if (!catalogs) return undefined;
  return Object.fromEntries(
    Object.entries(catalogs).map(([modelId, options]) => [
      modelId,
      selectConfigOptionsFromUnknown(options) ?? [],
    ]),
  );
}

function lookupModelBoundCatalog(
  selected: string | null | undefined,
  sessionCatalogs: Record<string, unknown[]> | null | undefined,
  authoringCatalogs: Record<string, AcpSelectConfigOptionVm[]> | null | undefined,
) {
  const modelId = selected?.trim();
  if (!modelId) return undefined;
  if (sessionCatalogs && Object.prototype.hasOwnProperty.call(sessionCatalogs, modelId)) {
    return sessionCatalogs[modelId];
  }
  if (authoringCatalogs && Object.prototype.hasOwnProperty.call(authoringCatalogs, modelId)) {
    return authoringCatalogs[modelId];
  }
  return undefined;
}

function hasModelBoundConfigRows(configOptions: unknown) {
  return (arrayValue(configOptions) ?? []).some((raw) => {
    const option = rawObject(raw);
    const category = stringValue(option?.category)?.trim() || stringValue(option?.id)?.trim();
    return isAcpModelBoundConfigCategory(category);
  });
}

function catalogModelCurrentValue(configOptions: unknown) {
  for (const raw of arrayValue(configOptions) ?? []) {
    const option = rawObject(raw);
    const id = stringValue(option?.id)?.trim();
    const category = stringValue(option?.category)?.trim();
    if (id === "model" || category === "model") {
      return stringValue(option?.currentValue)?.trim() || null;
    }
  }
  return null;
}

function spliceBoundOptions(configOptions: unknown, bound: unknown) {
  const options = arrayValue(configOptions) ?? [];
  const nonBound = options.filter((raw) => {
    const option = rawObject(raw);
    const category = stringValue(option?.category)?.trim() || stringValue(option?.id)?.trim();
    return !isAcpModelBoundConfigCategory(category);
  });
  const boundOptions = (arrayValue(bound) ?? []).map((raw) => {
    const option = rawObject(raw);
    if (!option || stringValue(option.type)?.trim()) return raw;
    return { ...option, type: "select" };
  });
  const modelIndex = nonBound.findIndex((raw) => {
    const option = rawObject(raw);
    return stringValue(option?.id)?.trim() === "model"
      || stringValue(option?.category)?.trim() === "model";
  });
  if (modelIndex < 0) return [...nonBound, ...boundOptions];
  return [
    ...nonBound.slice(0, modelIndex + 1),
    ...boundOptions,
    ...nonBound.slice(modelIndex + 1),
  ];
}

function mergeDoctorAndSessionConfigOptions(
  providerOptions: AcpSelectConfigOptionVm[],
  sessionOptions: unknown,
) {
  const doctorNonBound = mergeProviderCatalogCurrentValues(
    providerOptions.filter((option) => !isAcpModelBoundConfigCategory(option.category)),
    sessionOptions,
  );
  const sessionDependents = (arrayValue(sessionOptions) ?? []).flatMap((raw) => {
    const option = rawObject(raw);
    const id = stringValue(option?.id)?.trim();
    const category = stringValue(option?.category)?.trim() || id;
    if (!id || !isAcpModelBoundConfigCategory(category)) return [];
    const values = normalizeConfigOptionList(arrayValue(option?.options), category);
    if (values.length === 0) return [];
    return [{
      id,
      category,
      name: stringValue(option?.name),
      description: stringValue(option?.description),
      currentValue: stringValue(option?.currentValue),
      type: "select",
      options: values.map((value) => ({
        value: value.id,
        name: value.name,
        description: value.description,
      })),
    }];
  });
  const modelIndex = doctorNonBound.findIndex((option) => (
    option.id === "model" || option.category === "model"
  ));
  if (modelIndex < 0) return [...doctorNonBound, ...sessionDependents];
  return [
    ...doctorNonBound.slice(0, modelIndex + 1),
    ...sessionDependents,
    ...doctorNonBound.slice(modelIndex + 1),
  ];
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

function selectConfigOptionsFromUnknown(
  value: unknown,
): AcpSelectConfigOptionVm[] | undefined {
  const list = arrayValue(value);
  if (!list) return undefined;
  return list.flatMap((raw) => {
    const option = rawObject(raw);
    const id = stringValue(option?.id)?.trim();
    if (!id) return [];
    const category = stringValue(option?.category)?.trim() || id;
    const values = normalizeConfigOptionList(arrayValue(option?.options), category);
    return [{
      id,
      category,
      name: stringValue(option?.name),
      description: stringValue(option?.description),
      currentValue: stringValue(option?.currentValue),
      options: values.map((item) => ({
        value: item.id,
        name: item.name,
        description: item.description,
      })),
    }];
  });
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
    autoAccept: viewModel.autoAccept,
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
    modelBoundOptions: viewModel.modelBoundOptions.map((group) => ([
      group.id,
      group.category,
      group.currentValue,
      group.overrideValue,
      group.overrideValueName,
      group.canSelectUnspecified,
      group.options.map(signatureOption),
    ])),
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
