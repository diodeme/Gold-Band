import type { AcpSelectConfigOptionVm, AcpSelectConfigValueVm } from '@/types';

export const ACP_MODEL_CATEGORY = 'model';
export const ACP_THOUGHT_LEVEL_CATEGORY = 'thought_level';
export const ACP_MODEL_CONFIG_CATEGORY = 'model_config';

export type AcpCompositeConfigSection = {
  id: string;
  category: string;
  name: string | null;
  description: string | null;
  currentValue?: string | null;
  value: string | null;
  valueLabel?: string | null;
  showUnspecified?: boolean;
  options: Array<AcpSelectConfigValueVm & { available?: boolean }>;
};

export function isAcpModelBoundConfigCategory(category: string | null | undefined) {
  return category === ACP_THOUGHT_LEVEL_CATEGORY || category === ACP_MODEL_CONFIG_CATEGORY;
}

export function findAcpThoughtLevel(
  configOptions: AcpSelectConfigOptionVm[] | null | undefined,
) {
  return configOptions?.find((option) => option.category === ACP_THOUGHT_LEVEL_CATEGORY) ?? null;
}

export function findAcpModelConfigOptions(
  configOptions: AcpSelectConfigOptionVm[] | null | undefined,
) {
  return (configOptions ?? []).filter((option) => (
    option.category === ACP_MODEL_CONFIG_CATEGORY && option.options.length > 0
  ));
}

export function findAcpCatalogModelId(
  configOptions: ReadonlyArray<{
    id?: string | null;
    category?: string | null;
    currentValue?: string | null;
  }> | null | undefined,
) {
  const model = configOptions?.find((option) => (
    option.id === ACP_MODEL_CATEGORY || option.category === ACP_MODEL_CATEGORY
  ));
  const currentValue = model?.currentValue?.trim();
  return currentValue || null;
}

export function hasAuthoringModelBoundCatalog(
  modelBoundCatalogs: Record<string, AcpSelectConfigOptionVm[]> | null | undefined,
  modelId: string | null | undefined,
) {
  const selected = modelId?.trim();
  return Boolean(
    selected
    && modelBoundCatalogs
    && Object.prototype.hasOwnProperty.call(modelBoundCatalogs, selected),
  );
}

export function authoringConfigOptionsForModel(
  configOptions: AcpSelectConfigOptionVm[] | null | undefined,
  modelBoundCatalogs: Record<string, AcpSelectConfigOptionVm[]> | null | undefined,
  selectedModelId: string | null | undefined,
): AcpSelectConfigOptionVm[] {
  const base = configOptions ?? [];
  const nonBound = base.filter((option) => !isAcpModelBoundConfigCategory(option.category));
  const selected = selectedModelId?.trim() || findAcpCatalogModelId(base) || '';
  if (!selected) return base;
  if (hasAuthoringModelBoundCatalog(modelBoundCatalogs, selected)) {
    return [...nonBound, ...(modelBoundCatalogs?.[selected] ?? [])];
  }
  return base;
}

export function acpCompositeConfigSections(
  configOptions: AcpSelectConfigOptionVm[] | null | undefined,
  selectedModelId: string | null | undefined,
  values: Record<string, string> | null | undefined = undefined,
  modelBoundCatalogs: Record<string, AcpSelectConfigOptionVm[]> | null | undefined = undefined,
): AcpCompositeConfigSection[] {
  const projected = authoringConfigOptionsForModel(
    configOptions,
    modelBoundCatalogs,
    selectedModelId,
  );
  const remapped = remapAcpThoughtLevelOverride(values, projected);
  const bound = projected.filter((option) => (
    isAcpModelBoundConfigCategory(option.category)
    && option.options.length > 0
  ));
  const ordered = [
    ...bound.filter((option) => option.category === ACP_THOUGHT_LEVEL_CATEGORY),
    ...bound.filter((option) => option.category === ACP_MODEL_CONFIG_CATEGORY),
  ];
  return ordered.map((option) => {
    const value = remapped[option.id]?.trim() || null;
    return {
      id: option.id,
      category: option.category ?? option.id,
      name: option.name ?? null,
      description: option.description ?? null,
      currentValue: option.currentValue ?? null,
      value,
      valueLabel: option.options.find((candidate) => candidate.value === value)?.name ?? value,
      showUnspecified: true,
      options: option.options,
    };
  });
}

export function remapAcpThoughtLevelOverride(
  overrides: Record<string, string> | null | undefined,
  configOptions: AcpSelectConfigOptionVm[] | null | undefined,
): Record<string, string> {
  const next: Record<string, string> = { ...(overrides ?? {}) };
  const thought = findAcpThoughtLevel(configOptions);
  if (!thought) return next;
  const current = next[thought.id]?.trim();
  if (current && thought.options.some((option) => option.value === current)) {
    return next;
  }
  const catalogIds = new Set((configOptions ?? []).map((option) => option.id));
  for (const [optionId, value] of Object.entries(next)) {
    if (optionId === thought.id) continue;
    const catalogOption = (configOptions ?? []).find((option) => option.id === optionId);
    if (catalogIds.has(optionId) && catalogOption?.category !== ACP_THOUGHT_LEVEL_CATEGORY) {
      continue;
    }
    if (!thought.options.some((option) => option.value === value)) continue;
    delete next[optionId];
    next[thought.id] = value;
    break;
  }
  return next;
}

export function retainAcpModelBoundOverrides(
  overrides: Record<string, string> | null | undefined,
  configOptions: AcpSelectConfigOptionVm[] | null | undefined,
  selectedModelId?: string | null,
  modelBoundCatalogs: Record<string, AcpSelectConfigOptionVm[]> | null | undefined = undefined,
): Record<string, string> {
  const projected = authoringConfigOptionsForModel(
    configOptions,
    modelBoundCatalogs,
    selectedModelId,
  );
  const next = remapAcpThoughtLevelOverride(overrides, projected);
  const projectedIds = new Set(projected.map((option) => option.id));
  for (const option of projected) {
    if (!isAcpModelBoundConfigCategory(option.category)) continue;
    const value = next[option.id];
    if (value && !option.options.some((candidate) => candidate.value === value)) {
      delete next[option.id];
    }
  }
  for (const optionId of Object.keys(next)) {
    if (projectedIds.has(optionId)) continue;
    const leftover = (configOptions ?? []).find((option) => option.id === optionId)
      ?? Object.values(modelBoundCatalogs ?? {}).flat().find((option) => option.id === optionId);
    if (leftover && isAcpModelBoundConfigCategory(leftover.category)) {
      delete next[optionId];
    }
  }
  return next;
}

export type AcpConfigOptionNameTranslate = (
  key: string,
  options?: Record<string, unknown>,
) => string;

function sameLabelAsOptionId(name: string, optionId: string) {
  return name.localeCompare(optionId, undefined, { sensitivity: 'accent' }) === 0;
}

export function formatAcpConfigOptionLabel(
  name: string,
  optionId: string,
  translate?: AcpConfigOptionNameTranslate,
) {
  const label = name.trim();
  const id = optionId.trim();
  if (!label) return id;
  if (!id || sameLabelAsOptionId(label, id)) return label;
  const fallback = `${label}（${id}）`;
  if (!translate) return fallback;
  return translate('acp.configOptionWithId', {
    name: label,
    id,
    defaultValue: fallback,
  }).trim() || fallback;
}

export function acpCompositeSectionLabel(
  section: Pick<AcpCompositeConfigSection, 'id' | 'category' | 'name'>,
  thoughtLevelLabel: string,
  thoughtLevelCount = 1,
  translate?: AcpConfigOptionNameTranslate,
) {
  const fallback = section.name?.trim() || section.id;
  const mapped = section.category === ACP_THOUGHT_LEVEL_CATEGORY && thoughtLevelCount <= 1
    ? thoughtLevelLabel
    : translate
      ? translate(`acp.configOption.${section.id}`, { defaultValue: fallback }).trim() || fallback
      : fallback;
  return formatAcpConfigOptionLabel(mapped, section.id, translate);
}

export function acpShowsModelConfigSelect(
  models: { length: number } | null | undefined,
  configOptions: AcpSelectConfigOptionVm[] | null | undefined,
  selectedModelId: string | null | undefined,
  modelBoundCatalogs: Record<string, AcpSelectConfigOptionVm[]> | null | undefined = undefined,
) {
  return (models?.length ?? 0) > 0
    || acpCompositeConfigSections(
      configOptions,
      selectedModelId,
      undefined,
      modelBoundCatalogs,
    ).length > 0;
}
