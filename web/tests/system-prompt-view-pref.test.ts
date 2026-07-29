import { afterEach, describe, expect, it } from 'vitest';
import {
  DEFAULT_SYSTEM_PROMPT_VIEW_MODE,
  loadSystemPromptViewMode,
  saveSystemPromptViewMode,
  SYSTEM_PROMPT_VIEW_MODES,
  SYSTEM_PROMPT_VIEW_STORAGE_KEY,
} from '../src/lib/system-prompt-view-pref';

type Store = Record<string, string>;

function installMemoryLocalStorage(initial: Store = {}): Store {
  const store: Store = { ...initial };
  (globalThis as { localStorage?: Storage }).localStorage = {
    getItem: (key: string) => (key in store ? store[key] : null),
    setItem: (key: string, value: string) => {
      store[key] = value;
    },
    removeItem: (key: string) => {
      delete store[key];
    },
    clear: () => {
      for (const key of Object.keys(store)) delete store[key];
    },
    key: () => null,
    length: 0,
  } as Storage;
  return store;
}

describe('system prompt view preference', () => {
  afterEach(() => {
    delete (globalThis as { localStorage?: Storage }).localStorage;
  });

  it('defaults to rendered Markdown when no preference exists', () => {
    installMemoryLocalStorage();

    expect(DEFAULT_SYSTEM_PROMPT_VIEW_MODE).toBe(SYSTEM_PROMPT_VIEW_MODES.rendered);
    expect(loadSystemPromptViewMode()).toBe(SYSTEM_PROMPT_VIEW_MODES.rendered);
  });

  it('persists raw and rendered modes through the public preference API', () => {
    const store = installMemoryLocalStorage();

    saveSystemPromptViewMode(SYSTEM_PROMPT_VIEW_MODES.raw);
    expect(loadSystemPromptViewMode()).toBe(SYSTEM_PROMPT_VIEW_MODES.raw);
    expect(store[SYSTEM_PROMPT_VIEW_STORAGE_KEY]).toBe(SYSTEM_PROMPT_VIEW_MODES.raw);

    saveSystemPromptViewMode(SYSTEM_PROMPT_VIEW_MODES.rendered);
    expect(loadSystemPromptViewMode()).toBe(SYSTEM_PROMPT_VIEW_MODES.rendered);
  });

  it('falls back to rendered mode for missing, invalid, or unavailable storage', () => {
    installMemoryLocalStorage({ [SYSTEM_PROMPT_VIEW_STORAGE_KEY]: 'invalid' });
    expect(loadSystemPromptViewMode()).toBe(SYSTEM_PROMPT_VIEW_MODES.rendered);

    delete (globalThis as { localStorage?: Storage }).localStorage;
    expect(loadSystemPromptViewMode()).toBe(SYSTEM_PROMPT_VIEW_MODES.rendered);
    expect(() => saveSystemPromptViewMode(SYSTEM_PROMPT_VIEW_MODES.raw)).not.toThrow();
  });

  it('keeps the in-memory choice usable when persistence fails', () => {
    (globalThis as { localStorage?: Storage }).localStorage = {
      getItem: () => null,
      setItem: () => {
        throw new Error('quota exceeded');
      },
      removeItem: () => {},
      clear: () => {},
      key: () => null,
      length: 0,
    } as Storage;

    expect(() => saveSystemPromptViewMode(SYSTEM_PROMPT_VIEW_MODES.raw)).not.toThrow();
  });
});
