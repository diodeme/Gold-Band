export const PERSONAL_ANALYTICS_PREFERENCES_STORAGE_KEY = 'gold-band:personal-analytics-preferences';

const PERSONAL_ANALYTICS_PREFERENCES_SCHEMA_VERSION = 2;
const MAX_REMEMBERED_AGENTS = 32;

interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

interface RememberedAgentSelection {
  modelId: string;
  thoughtLevelOptionId: string;
  thoughtLevelValue: string;
}

interface PersonalAnalyticsPreferencesV2 {
  schemaVersion: typeof PERSONAL_ANALYTICS_PREFERENCES_SCHEMA_VERSION;
  agentType: string;
  selectionByAgent: Record<string, RememberedAgentSelection>;
}

interface AgentCapability {
  agentType: string;
  supportedModels?: ReadonlyArray<{ id: string }> | null;
  configOptions?: ReadonlyArray<{
    id: string;
    category?: string | null;
    options: ReadonlyArray<{ value: string }>;
  }> | null;
}

export interface PersonalAnalyticsSelection {
  agentType: string;
  modelId: string;
  thoughtLevelOptionId: string;
  thoughtLevelValue: string;
}

export function resolvePersonalAnalyticsSelection(
  availableAgents: readonly AgentCapability[],
  requestedAgentType?: string,
  storage: StorageLike | null = browserStorage(),
): PersonalAnalyticsSelection {
  const preferences = readPreferences(storage);
  const requested = requestedAgentType
    ? availableAgents.find((agent) => agent.agentType === requestedAgentType)
    : undefined;
  const remembered = availableAgents.find((agent) => agent.agentType === preferences.agentType);
  const agent = requested ?? remembered ?? availableAgents[0];
  if (!agent) return emptySelection('');

  const rememberedSelection = preferences.selectionByAgent[agent.agentType];
  const rememberedModelId = rememberedSelection?.modelId ?? '';
  const modelId = agent.supportedModels?.some((model) => model.id === rememberedModelId)
    ? rememberedModelId
    : '';
  const thoughtLevel = agent.configOptions?.find((option) => option.category === 'thought_level');
  const thoughtSelectionValid = Boolean(
    thoughtLevel
    && rememberedSelection?.thoughtLevelOptionId === thoughtLevel.id
    && thoughtLevel.options.some((option) => option.value === rememberedSelection.thoughtLevelValue),
  );
  return {
    agentType: agent.agentType,
    modelId,
    thoughtLevelOptionId: thoughtSelectionValid ? rememberedSelection!.thoughtLevelOptionId : '',
    thoughtLevelValue: thoughtSelectionValid ? rememberedSelection!.thoughtLevelValue : '',
  };
}

export function rememberPersonalAnalyticsSelection(
  selection: PersonalAnalyticsSelection,
  storage: StorageLike | null = browserStorage(),
) {
  if (!storage || !selection.agentType) return false;
  try {
    const preferences = readPreferences(storage);
    const selectionByAgent = { ...preferences.selectionByAgent };
    delete selectionByAgent[selection.agentType];
    selectionByAgent[selection.agentType] = normalizedSelection(selection);
    const boundedEntries = Object.entries(selectionByAgent).slice(-MAX_REMEMBERED_AGENTS);
    storage.setItem(PERSONAL_ANALYTICS_PREFERENCES_STORAGE_KEY, JSON.stringify({
      schemaVersion: PERSONAL_ANALYTICS_PREFERENCES_SCHEMA_VERSION,
      agentType: selection.agentType,
      selectionByAgent: Object.fromEntries(boundedEntries),
    } satisfies PersonalAnalyticsPreferencesV2));
    return true;
  } catch {
    return false;
  }
}

function readPreferences(storage: StorageLike | null): PersonalAnalyticsPreferencesV2 {
  if (!storage) return emptyPreferences();
  try {
    const parsed = JSON.parse(storage.getItem(PERSONAL_ANALYTICS_PREFERENCES_STORAGE_KEY) ?? 'null') as Partial<PersonalAnalyticsPreferencesV2> | null;
    if (
      parsed?.schemaVersion !== PERSONAL_ANALYTICS_PREFERENCES_SCHEMA_VERSION
      || typeof parsed.agentType !== 'string'
      || !parsed.selectionByAgent
      || typeof parsed.selectionByAgent !== 'object'
      || Array.isArray(parsed.selectionByAgent)
    ) {
      return emptyPreferences();
    }
    const selectionByAgent = Object.fromEntries(
      Object.entries(parsed.selectionByAgent)
        .filter((entry): entry is [string, RememberedAgentSelection] => Boolean(entry[0]) && validStoredSelection(entry[1]))
        .slice(-MAX_REMEMBERED_AGENTS),
    );
    return {
      schemaVersion: PERSONAL_ANALYTICS_PREFERENCES_SCHEMA_VERSION,
      agentType: parsed.agentType,
      selectionByAgent,
    };
  } catch {
    return emptyPreferences();
  }
}

function validStoredSelection(value: unknown): value is RememberedAgentSelection {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const selection = value as Partial<RememberedAgentSelection>;
  return typeof selection.modelId === 'string'
    && typeof selection.thoughtLevelOptionId === 'string'
    && typeof selection.thoughtLevelValue === 'string'
    && Boolean(selection.thoughtLevelOptionId) === Boolean(selection.thoughtLevelValue);
}

function normalizedSelection(selection: PersonalAnalyticsSelection): RememberedAgentSelection {
  const hasThoughtLevel = Boolean(selection.thoughtLevelOptionId && selection.thoughtLevelValue);
  return {
    modelId: selection.modelId,
    thoughtLevelOptionId: hasThoughtLevel ? selection.thoughtLevelOptionId : '',
    thoughtLevelValue: hasThoughtLevel ? selection.thoughtLevelValue : '',
  };
}

function emptySelection(agentType: string): PersonalAnalyticsSelection {
  return { agentType, modelId: '', thoughtLevelOptionId: '', thoughtLevelValue: '' };
}

function emptyPreferences(): PersonalAnalyticsPreferencesV2 {
  return {
    schemaVersion: PERSONAL_ANALYTICS_PREFERENCES_SCHEMA_VERSION,
    agentType: '',
    selectionByAgent: {},
  };
}

function browserStorage(): StorageLike | null {
  if (typeof window === 'undefined') return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
