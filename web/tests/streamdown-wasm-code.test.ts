import { describe, expect, it, vi } from 'vitest';
import type { CodeHighlighterPlugin } from 'streamdown';
import { createWasmCodePlugin } from '@/lib/streamdown-wasm-code';

function runtimeFixture(options: { fail?: boolean } = {}) {
  const highlighter = {
    getLoadedLanguages: vi.fn(() => [] as string[]),
    loadLanguage: vi.fn(async () => {}),
    codeToTokens: vi.fn((code: string) => ({
      tokens: [[{ content: code, color: '#fff' }]],
    })),
  };
  const createHighlighter = vi.fn(async () => {
    if (options.fail) throw new Error('wasm unavailable');
    return highlighter;
  });
  return {
    highlighter,
    createHighlighter,
    runtime: {
      bundledLanguages: { typescript: {} },
      bundledLanguagesInfo: [{ id: 'typescript', aliases: ['ts'] }],
      createHighlighter,
    },
  };
}

function highlightAsync(plugin: CodeHighlighterPlugin, code: string, language = 'ts') {
  return new Promise<NonNullable<ReturnType<CodeHighlighterPlugin['highlight']>>>((resolve) => {
    const immediate = plugin.highlight({
      code,
      language: language as never,
      themes: ['github-light', 'github-dark'],
    }, resolve);
    if (immediate) resolve(immediate);
  });
}

describe('Streamdown WASM code plugin', () => {
  it('initializes Shiki lazily once and reuses the highlighted result', async () => {
    const fixture = runtimeFixture();
    const plugin = createWasmCodePlugin({}, fixture.runtime as never);

    expect(fixture.createHighlighter).not.toHaveBeenCalled();
    const first = await highlightAsync(plugin, 'const value = 1;');
    const second = await highlightAsync(plugin, 'const value = 1;');

    expect(first).toEqual(second);
    expect(fixture.createHighlighter).toHaveBeenCalledTimes(1);
    expect(fixture.highlighter.loadLanguage).toHaveBeenCalledTimes(1);
    expect(fixture.highlighter.codeToTokens).toHaveBeenCalledTimes(1);
  });

  it('coalesces concurrent requests for the same code block', async () => {
    const fixture = runtimeFixture();
    const plugin = createWasmCodePlugin({}, fixture.runtime as never);

    const [first, second] = await Promise.all([
      highlightAsync(plugin, 'let pending = true;'),
      highlightAsync(plugin, 'let pending = true;'),
    ]);

    expect(first).toEqual(second);
    expect(fixture.createHighlighter).toHaveBeenCalledTimes(1);
    expect(fixture.highlighter.codeToTokens).toHaveBeenCalledTimes(1);
  });

  it('falls back to plain tokens and reports initialization failure once', async () => {
    const fixture = runtimeFixture({ fail: true });
    const onInitializationError = vi.fn();
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    const plugin = createWasmCodePlugin({ onInitializationError }, fixture.runtime as never);

    const result = await highlightAsync(plugin, 'plain\ncode');
    const cached = await highlightAsync(plugin, 'plain\ncode');

    expect(result.tokens).toEqual([[{ content: 'plain' }], [{ content: 'code' }]]);
    expect(cached).toEqual(result);
    expect(onInitializationError).toHaveBeenCalledTimes(1);
    expect(consoleError).toHaveBeenCalledTimes(1);
    consoleError.mockRestore();
  });
});
