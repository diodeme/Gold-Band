import {
  bundledLanguages,
  bundledLanguagesInfo,
  createHighlighter,
  type BundledLanguage,
  type BundledTheme,
  type Highlighter,
} from 'shiki/bundle/web';
import type { CodeHighlighterPlugin } from 'streamdown';

type HighlightResult = NonNullable<ReturnType<CodeHighlighterPlugin['highlight']>>;
type HighlightCallback = NonNullable<Parameters<CodeHighlighterPlugin['highlight']>[1]>;
type ThemePair = [BundledTheme, BundledTheme];

interface WasmShikiRuntime {
  readonly bundledLanguages: typeof bundledLanguages;
  readonly bundledLanguagesInfo: typeof bundledLanguagesInfo;
  createHighlighter(options: Parameters<typeof createHighlighter>[0]): Promise<Highlighter>;
}

export interface WasmCodePluginOptions {
  readonly themes?: ThemePair;
  readonly cacheSize?: number;
  readonly onInitializationError?: (error: unknown) => void;
}

const DEFAULT_THEMES = ['github-light', 'github-dark'] as ThemePair;
const DEFAULT_CACHE_SIZE = 128;
const defaultRuntime: WasmShikiRuntime = {
  bundledLanguages,
  bundledLanguagesInfo,
  createHighlighter,
};

function boundedCacheSet(cache: Map<string, HighlightResult>, key: string, value: HighlightResult, limit: number) {
  if (cache.has(key)) cache.delete(key);
  cache.set(key, value);
  while (cache.size > limit) {
    const oldest = cache.keys().next().value;
    if (oldest === undefined) break;
    cache.delete(oldest);
  }
}

function plainHighlightResult(code: string): HighlightResult {
  return {
    tokens: code.split('\n').map((line) => [{ content: line }]),
  };
}

export function createWasmCodePlugin(
  options: WasmCodePluginOptions = {},
  runtime: WasmShikiRuntime = defaultRuntime,
): CodeHighlighterPlugin {
  const themes = options.themes ?? DEFAULT_THEMES;
  const cacheSize = Math.max(1, options.cacheSize ?? DEFAULT_CACHE_SIZE);
  const aliases = new Map<string, BundledLanguage>();
  const languages = new Set(Object.keys(runtime.bundledLanguages) as BundledLanguage[]);
  for (const language of runtime.bundledLanguagesInfo) {
    for (const alias of language.aliases ?? []) {
      aliases.set(alias.toLowerCase(), language.id as BundledLanguage);
    }
  }

  const results = new Map<string, HighlightResult>();
  const pendingCallbacks = new Map<string, Set<HighlightCallback>>();
  let highlighterPromise: Promise<Highlighter> | null = null;
  let initializationErrorReported = false;

  const normalizeLanguage = (language: string) => {
    const normalized = language.trim().toLowerCase();
    return aliases.get(normalized) ?? normalized as BundledLanguage;
  };

  const highlighter = () => {
    highlighterPromise ??= runtime.createHighlighter({ themes: [...themes], langs: [] });
    return highlighterPromise;
  };

  const publish = (key: string, result: HighlightResult) => {
    boundedCacheSet(results, key, result, cacheSize);
    const callbacks = pendingCallbacks.get(key);
    pendingCallbacks.delete(key);
    for (const callback of callbacks ?? []) callback(result);
  };

  return {
    name: 'shiki',
    type: 'code-highlighter',
    getSupportedLanguages: () => Array.from(languages),
    getThemes: () => themes,
    supportsLanguage: (language) => languages.has(normalizeLanguage(language)),
    highlight({ code, language, themes: requestedThemes }, callback) {
      const normalizedLanguage = normalizeLanguage(language);
      const lightTheme = typeof requestedThemes[0] === 'string'
        ? requestedThemes[0] as BundledTheme
        : themes[0];
      const darkTheme = typeof requestedThemes[1] === 'string'
        ? requestedThemes[1] as BundledTheme
        : themes[1];
      const key = `${normalizedLanguage}\u0000${lightTheme}\u0000${darkTheme}\u0000${code}`;
      const cached = results.get(key);
      if (cached) {
        results.delete(key);
        results.set(key, cached);
        return cached;
      }
      if (callback) {
        const callbacks = pendingCallbacks.get(key) ?? new Set<HighlightCallback>();
        callbacks.add(callback);
        pendingCallbacks.set(key, callbacks);
      }
      if (pendingCallbacks.get(key)?.size === 1 || !callback) {
        void highlighter().then(async (instance) => {
          const languageToLoad = languages.has(normalizedLanguage) ? normalizedLanguage : null;
          if (languageToLoad && !instance.getLoadedLanguages().includes(languageToLoad)) {
            await instance.loadLanguage(languageToLoad);
          }
          const result = languageToLoad
            ? instance.codeToTokens(code, {
              lang: languageToLoad,
              themes: { light: lightTheme, dark: darkTheme },
            })
            : plainHighlightResult(code);
          publish(key, result);
        }).catch((error) => {
          if (!initializationErrorReported) {
            initializationErrorReported = true;
            options.onInitializationError?.(error);
            console.error('[Gold Band WebView] WASM code highlighter unavailable', error);
          }
          publish(key, plainHighlightResult(code));
        });
      }
      return null;
    },
  };
}

export const wasmCode = createWasmCodePlugin();
