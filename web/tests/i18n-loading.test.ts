import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';
import i18n, { loadI18nLanguage } from '@/i18n';
import en from '@/locales/en.json';
import es from '@/locales/es.json';
import jaJP from '@/locales/ja-JP.json';
import koKR from '@/locales/ko-KR.json';
import ptBR from '@/locales/pt-BR.json';
import zhCN from '@/locales/zh-CN.json';
import zhTW from '@/locales/zh-TW.json';

const localeCatalogs = {
  'zh-CN': zhCN,
  'zh-TW': zhTW,
  en,
  'ja-JP': jaJP,
  'ko-KR': koKR,
  'pt-BR': ptBR,
  es,
};

async function restoreTestCatalogs() {
  for (const [locale, catalog] of Object.entries(localeCatalogs)) {
    i18n.addResourceBundle(locale, 'translation', catalog, true, true);
  }
  i18n.removeResourceBundle('en', 'fallback-test');
  i18n.removeResourceBundle('es', 'fallback-test');
  await i18n.changeLanguage('zh-CN');
}

function leafKeys(value: unknown, prefix = ''): string[] {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return [prefix];
  return Object.entries(value).flatMap(([key, child]) =>
    leafKeys(child, prefix ? `${prefix}.${key}` : key));
}

describe('UI locale loading contract', () => {
  afterEach(restoreTestCatalogs);

  it('keeps every locale key set identical to English', () => {
    const baseline = leafKeys(en).sort();
    for (const [locale, catalog] of Object.entries(localeCatalogs)) {
      expect(leafKeys(catalog).sort(), locale).toEqual(baseline);
    }
  });

  it('keeps locale catalogs behind dynamic imports', () => {
    const source = readFileSync(
      fileURLToPath(new URL('../src/i18n.ts', import.meta.url)),
      'utf8',
    );

    expect(source).not.toMatch(/import\s+\w+\s+from\s+['"]\.\/locales\//);
    for (const locale of Object.keys(localeCatalogs)) {
      expect(source).toContain(`import(\"./locales/${locale}.json\")`);
    }
  });

  it('loads the canonical language before publishing bootstrap and saved preferences', () => {
    const source = readFileSync(
      fileURLToPath(new URL('../src/App.tsx', import.meta.url)),
      'utf8',
    );
    const bootstrapLoad = source.indexOf(
      'const tag = await loadI18nLanguage(bootstrap.preferences.language);',
    );
    const bootstrapPublish = source.indexOf('setBootstrap(bootstrap);', bootstrapLoad);
    const savedLoad = source.indexOf(
      'const tag = await loadI18nLanguage(saved.language);',
      bootstrapPublish,
    );
    const savedPublish = source.indexOf(
      'setBootstrap((current) => current ? { ...current, preferences: saved } : current);',
      savedLoad,
    );

    expect(bootstrapLoad).toBeGreaterThan(-1);
    expect(bootstrapPublish).toBeGreaterThan(bootstrapLoad);
    expect(savedLoad).toBeGreaterThan(bootstrapPublish);
    expect(savedPublish).toBeGreaterThan(savedLoad);
  });

  it('registers only the requested locale when the resource store is empty', async () => {
    for (const locale of Object.keys(localeCatalogs)) {
      i18n.removeResourceBundle(locale, 'translation');
    }

    await loadI18nLanguage('ja-jp');

    expect(i18n.language).toBe('ja-JP');
    for (const locale of Object.keys(localeCatalogs)) {
      expect(i18n.hasResourceBundle(locale, 'translation'), locale).toBe(locale === 'ja-JP');
    }
  });

  it('does not resolve an ordinary missing key from English', async () => {
    i18n.addResourceBundle('en', 'fallback-test', { onlyInEnglish: 'English fallback' });
    i18n.addResourceBundle('es', 'fallback-test', {});
    await i18n.changeLanguage('es');

    expect(i18n.options.fallbackLng).toBe(false);
    expect(i18n.t('onlyInEnglish', { ns: 'fallback-test' })).toBe('onlyInEnglish');
  });
});

const reservedTerms = [
  { name: 'CI/CD', matches: (text: string) => text.includes('CI/CD') },
  { name: 'GitHub', matches: (text: string) => text.includes('GitHub') },
  { name: 'Agent', matches: (text: string) => /\bAgents?\b/i.test(text) },
  { name: 'worktree', matches: (text: string) => /\bworktrees?\b/i.test(text) },
  { name: 'Workflow', matches: (text: string) => /\bWorkflows?\b/i.test(text) },
  { name: 'MCP', matches: (text: string) => /\bMCP\b/.test(text) },
  { name: 'ACP', matches: (text: string) => /\bACP\b/.test(text) },
  { name: 'Git', matches: (text: string) => /\bGit\b/i.test(text) },
];

function leafText(value: unknown, prefix = ''): Array<[string, string]> {
  if (typeof value === 'string') return [[prefix, value]];
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`locale leaf ${prefix || '<root>'} is not a string`);
  }
  return Object.entries(value).flatMap(([key, child]) =>
    leafText(child, prefix ? `${prefix}.${key}` : key));
}

function placeholdersOf(text: string): string[] {
  return (text.match(/\{\{[^}]+\}\}/g) ?? []).sort();
}

describe('UI locale catalog contract', () => {
  const source = new Map(leafText(zhCN));
  const localized = Object.entries(localeCatalogs)
    .filter(([locale]) => locale !== 'zh-CN')
    .map(([locale, catalog]) => [locale, new Map(leafText(catalog))] as const);

  it('keeps placeholders identical to zh-CN for every key', () => {
    const mismatches: string[] = [];
    for (const [key, sourceText] of source) {
      const expected = placeholdersOf(sourceText).join('|');
      for (const [locale, catalog] of localized) {
        const actual = placeholdersOf(catalog.get(key) ?? '').join('|');
        if (actual !== expected) mismatches.push(`${locale} ${key}`);
      }
    }
    expect(mismatches).toEqual([]);
  });

  it('keeps reserved English terms when zh-CN keeps them', () => {
    const violations: string[] = [];
    for (const [key, sourceText] of source) {
      const required = reservedTerms.filter((term) => term.matches(sourceText));
      if (required.length === 0) continue;
      for (const [locale, catalog] of localized) {
        const text = catalog.get(key) ?? '';
        for (const term of required) {
          if (!term.matches(text)) violations.push(`${locale} ${key} missing ${term.name}`);
        }
      }
    }
    expect(violations).toEqual([]);
  });
});
