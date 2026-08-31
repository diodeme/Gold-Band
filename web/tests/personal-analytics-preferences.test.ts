import { describe, expect, it } from 'vitest';
import {
  PERSONAL_ANALYTICS_PREFERENCES_STORAGE_KEY,
  rememberPersonalAnalyticsSelection,
  resolvePersonalAnalyticsSelection,
} from '../src/lib/personal-analytics-preferences';

class MemoryStorage {
  private readonly values = new Map<string, string>();

  getItem(key: string) {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string) {
    this.values.set(key, value);
  }
}

const agents = [
  {
    agentType: 'agent-a',
    supportedModels: [{ id: 'model-a' }, { id: 'model-a-deep' }],
    configOptions: [{
      id: 'reasoning_effort',
      category: 'thought_level',
      options: [{ value: 'low' }, { value: 'high' }],
    }],
  },
  {
    agentType: 'agent-b',
    supportedModels: [{ id: 'model-b' }],
    configOptions: [{
      id: 'thinking_budget',
      category: 'thought_level',
      options: [{ value: 'standard' }],
    }],
  },
];

describe('personal analytics preferences', () => {
  it('remembers a separate model and thought level for each Agent', () => {
    const storage = new MemoryStorage();

    rememberPersonalAnalyticsSelection({
      agentType: 'agent-a',
      modelId: 'model-a-deep',
      thoughtLevelOptionId: 'reasoning_effort',
      thoughtLevelValue: 'high',
    }, storage);
    rememberPersonalAnalyticsSelection({
      agentType: 'agent-b',
      modelId: 'model-b',
      thoughtLevelOptionId: 'thinking_budget',
      thoughtLevelValue: 'standard',
    }, storage);

    expect(resolvePersonalAnalyticsSelection(agents, undefined, storage)).toEqual({
      agentType: 'agent-b',
      modelId: 'model-b',
      thoughtLevelOptionId: 'thinking_budget',
      thoughtLevelValue: 'standard',
    });
    expect(resolvePersonalAnalyticsSelection(agents, 'agent-a', storage)).toEqual({
      agentType: 'agent-a',
      modelId: 'model-a-deep',
      thoughtLevelOptionId: 'reasoning_effort',
      thoughtLevelValue: 'high',
    });
  });

  it('rejects unsupported schemas and stale Agent, model, or thought-level identities', () => {
    const storage = new MemoryStorage();
    storage.setItem(PERSONAL_ANALYTICS_PREFERENCES_STORAGE_KEY, JSON.stringify({
      schemaVersion: 99,
      agentType: 'agent-b',
      modelByAgent: { 'agent-b': 'model-b' },
    }));
    expect(resolvePersonalAnalyticsSelection(agents, undefined, storage)).toEqual({
      agentType: 'agent-a',
      modelId: '',
      thoughtLevelOptionId: '',
      thoughtLevelValue: '',
    });

    storage.setItem(PERSONAL_ANALYTICS_PREFERENCES_STORAGE_KEY, JSON.stringify({
      schemaVersion: 2,
      agentType: 'removed-agent',
      selectionByAgent: {
        'agent-a': {
          modelId: 'removed-model',
          thoughtLevelOptionId: 'reasoning_effort',
          thoughtLevelValue: 'removed-level',
        },
      },
    }));
    expect(resolvePersonalAnalyticsSelection(agents, undefined, storage)).toEqual({
      agentType: 'agent-a',
      modelId: '',
      thoughtLevelOptionId: '',
      thoughtLevelValue: '',
    });
  });

  it('bounds remembered per-Agent selection entries', () => {
    const storage = new MemoryStorage();
    for (let index = 0; index < 48; index += 1) {
      rememberPersonalAnalyticsSelection({
        agentType: `agent-${index}`,
        modelId: `model-${index}`,
        thoughtLevelOptionId: '',
        thoughtLevelValue: '',
      }, storage);
    }

    const persisted = JSON.parse(storage.getItem(PERSONAL_ANALYTICS_PREFERENCES_STORAGE_KEY) ?? '{}') as {
      selectionByAgent?: Record<string, unknown>;
    };
    expect(Object.keys(persisted.selectionByAgent ?? {})).toHaveLength(32);
  });
});
