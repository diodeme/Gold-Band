import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  ARTIFACT_MARKDOWN_RENDER_STORAGE_KEY,
  DEFAULT_ARTIFACT_MARKDOWN_RENDER,
  loadArtifactMarkdownRender,
  saveArtifactMarkdownRender,
} from '../src/lib/artifact-markdown-pref';

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

describe('artifact markdown render preference', () => {
  afterEach(() => {
    delete (globalThis as { localStorage?: Storage }).localStorage;
  });

  it('defaults to rendering markdown when no preference is stored', () => {
    installMemoryLocalStorage();
    expect(DEFAULT_ARTIFACT_MARKDOWN_RENDER).toBe(true);
    expect(loadArtifactMarkdownRender()).toBe(true);
  });

  it('persists and reloads the disabled (raw) state', () => {
    installMemoryLocalStorage();
    saveArtifactMarkdownRender(false);
    expect(loadArtifactMarkdownRender()).toBe(false);
  });

  it('persists and reloads the enabled (rendered) state', () => {
    installMemoryLocalStorage();
    saveArtifactMarkdownRender(false); // flip first
    saveArtifactMarkdownRender(true);
    expect(loadArtifactMarkdownRender()).toBe(true);
  });

  it('writes the canonical storage key as a boolean string', () => {
    const store = installMemoryLocalStorage();
    saveArtifactMarkdownRender(false);
    expect(store[ARTIFACT_MARKDOWN_RENDER_STORAGE_KEY]).toBe('false');
  });

  it('falls back to the default for unrecognized stored values', () => {
    installMemoryLocalStorage({ [ARTIFACT_MARKDOWN_RENDER_STORAGE_KEY]: 'garbage' });
    expect(loadArtifactMarkdownRender()).toBe(true);
  });

  it('is resilient when localStorage is unavailable', () => {
    // No localStorage installed on globalThis.
    expect(loadArtifactMarkdownRender()).toBe(true);
    expect(() => saveArtifactMarkdownRender(false)).not.toThrow();
  });

  it('swallows persistence errors instead of throwing', () => {
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
    expect(() => saveArtifactMarkdownRender(false)).not.toThrow();
  });
});
