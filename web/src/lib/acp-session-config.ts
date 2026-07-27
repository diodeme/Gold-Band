import type { AcpSessionConfigVm } from "@/types";

export type AcpSessionConfigCategory = string;

export type AcpSessionConfigOption = {
  id: string;
  name: string;
  description?: string | null;
};

export type AcpSessionConfigGroup = {
  id: string;
  category: string;
  name: string | null;
  description: string | null;
  currentValue: string | null;
  overrideValue: string | null;
  options: AcpSessionConfigOption[];
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
): AcpSessionConfigViewModel {
  const currentModelId = config?.currentModelId ?? null;
  const currentModelName = config?.currentModelName ?? null;
  const currentModeId = config?.currentModeId ?? null;
  const currentModeName = config?.currentModeName ?? null;
  const availableModels = normalizeAcpSessionConfigOptions(
    config?.models,
    config?.configOptions,
    "model",
  );
  const availablePermissionModes = normalizeAcpSessionConfigOptions(
    config?.modes,
    config?.configOptions,
    "mode",
  );
  const modelOverrideId = config?.modelOverrideId ?? null;
  const modelOverrideName = modelOverrideId
    ? availableModels.find((option) => option.id === modelOverrideId)?.name
      ?? (currentModelId === modelOverrideId ? currentModelName : null)
      ?? modelOverrideId
    : null;
  const permissionModeOverrideId = config?.permissionModeOverrideId ?? null;
  const thoughtLevel = normalizeAcpSelectConfigGroups(
    config?.configOptions,
    config?.configOptionOverrides,
  ).find((group) => group.category === "thought_level") ?? null;
  const permissionModeOverrideName = permissionModeOverrideId
    ? availablePermissionModes.find((option) => option.id === permissionModeOverrideId)?.name
      ?? (currentModeId === permissionModeOverrideId ? currentModeName : null)
      ?? permissionModeOverrideId
    : null;
  const resolvedCurrentModelName = currentModelName ?? (
    currentModelId ? null : singleOptionName(availableModels)
  );
  const resolvedCurrentModeName = currentModeName ?? (
    currentModeId ? null : singleOptionName(availablePermissionModes)
  );
  const resolvedModeLabel = resolvedCurrentModeName ?? currentModeId;
  const viewModel = {
    modelOverrideId,
    modelOverrideName,
    canSelectUnspecifiedModel: true,
    permissionModeOverrideId,
    permissionModeOverrideName,
    canSelectUnspecifiedPermissionMode: true,
    currentModelId,
    currentModelName: resolvedCurrentModelName,
    currentModeId,
    currentModeName: resolvedCurrentModeName,
    modeLabel: resolvedModeLabel,
    availableModels,
    availablePermissionModes,
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
): AcpSessionConfigGroup[] {
  return (arrayValue(configOptions) ?? []).flatMap((raw) => {
    const option = rawObject(raw);
    const id = stringValue(option?.id)?.trim();
    if (!id || stringValue(option?.type) !== "select") return [];
    const category = stringValue(option?.category)?.trim() || id;
    const options = normalizeConfigOptionList(arrayValue(option?.options), category);
    if (options.length === 0) return [];
    return [{
      id,
      category,
      name: stringValue(option?.name)?.trim() || null,
      description: stringValue(option?.description)?.trim() || null,
      currentValue: stringValue(option?.currentValue)?.trim() || null,
      overrideValue: overrides?.[id]?.trim() || null,
      options,
    }];
  });
}

export function findAcpConfigOption(
  groupedOptions: unknown,
  configOptions: unknown,
  category: AcpSessionConfigCategory,
  id: string,
): AcpSessionConfigOption {
  const groupedMatch = groupedConfigOptions(groupedOptions, category).find(
    (option) => option.id === id,
  );
  if (groupedMatch) return groupedMatch;

  const configMatch = configOptionValues(configOptions, category).find(
    (option) => option.id === id,
  );
  return configMatch ?? { id, name: id };
}

export function normalizeAcpSessionConfigOptions(
  groupedOptions: unknown,
  configOptions: unknown,
  category: AcpSessionConfigCategory,
): AcpSessionConfigOption[] {
  const grouped = groupedConfigOptions(groupedOptions, category);
  if (grouped.length > 0) return grouped;
  return configOptionValues(configOptions, category);
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
      options: viewModel.thoughtLevel.options.map(signatureOption),
    } : null,
  });
}

function signatureOption(option: AcpSessionConfigOption) {
  return [option.id, option.name, option.description ?? null];
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
