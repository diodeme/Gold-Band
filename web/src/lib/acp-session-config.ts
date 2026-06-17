import type { AcpSessionConfigVm } from "@/types";

export type AcpSessionConfigCategory = "model" | "mode";

export type AcpSessionConfigOption = {
  id: string;
  name: string;
  description?: string | null;
};

export type AcpSessionConfigViewModel = {
  currentModelId: string | null;
  currentModelName: string | null;
  currentModeId: string | null;
  currentModeName: string | null;
  modeLabel: string | null;
  availableModels: AcpSessionConfigOption[];
  availablePermissionModes: AcpSessionConfigOption[];
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
  const resolvedCurrentModelName = currentModelName ?? (
    currentModelId ? null : singleOptionName(availableModels)
  );
  const resolvedCurrentModeName = currentModeName ?? (
    currentModeId ? null : singleOptionName(availablePermissionModes)
  );
  const resolvedModeLabel = resolvedCurrentModeName ?? currentModeId;
  const viewModel = {
    currentModelId,
    currentModelName: resolvedCurrentModelName,
    currentModeId,
    currentModeName: resolvedCurrentModeName,
    modeLabel: resolvedModeLabel,
    availableModels,
    availablePermissionModes,
  };

  return {
    ...viewModel,
    signature: createAcpSessionConfigSignature(viewModel),
  };
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
    currentModelId: viewModel.currentModelId,
    currentModelName: viewModel.currentModelName,
    currentModeId: viewModel.currentModeId,
    currentModeName: viewModel.currentModeName,
    models: viewModel.availableModels.map(signatureOption),
    modes: viewModel.availablePermissionModes.map(signatureOption),
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
