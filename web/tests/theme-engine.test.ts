import { readFileSync } from 'node:fs';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AppearancePreference } from '../src/types';
import { themePackageSchema } from '../src/theme-contract';
import { builtinThemes } from '../src/themes/builtin-themes';
import {
  appearanceWithQuality,
  appearanceWithTheme,
  applyAppearance,
  defaultAppearancePreference,
  getThemePackage,
  normalizeAppearancePreference,
  resolveAppearance,
  resolveColorScheme,
  themePackageSummaries,
} from '../src/theme';

const preference = (overrides: Partial<AppearancePreference> = {}): AppearancePreference => ({
  ...defaultAppearancePreference,
  colorScheme: 'dark',
  ...overrides,
});

const generatedThemeCss = readFileSync(
  path.resolve(__dirname, '../src/themes/generated/builtin-themes.css'),
  'utf8',
);

function generatedRoleRule(themeId: string, role: string): string {
  const selector = `:root[data-theme='${themeId}'] [data-theme-role='${role}']`;
  const start = generatedThemeCss.indexOf(selector);
  const end = generatedThemeCss.indexOf('}', start);
  expect(start, `${themeId}/${role} generated rule`).toBeGreaterThanOrEqual(0);
  return generatedThemeCss.slice(start, end + 1);
}

describe('theme package contract', () => {
  it('validates every built-in package with paired light and dark schemes', () => {
    expect(builtinThemes).toHaveLength(4);
    for (const theme of builtinThemes) {
      expect(themePackageSchema.parse(theme)).toStrictEqual(theme);
      expect(theme.schemes.light).toBeDefined();
      expect(theme.schemes.dark).toBeDefined();
    }
    expect(themePackageSummaries.map(({ id }) => id)).toEqual([
      'builtin.gold-band',
      'builtin.glass',
      'builtin.neo-brutalist',
      'builtin.tech-neutral',
    ]);
  });

  it('registers a high-difference declarative package through the shared contract', () => {
    const goldBand = getThemePackage('builtin.gold-band');
    const neoBrutalist = getThemePackage('builtin.neo-brutalist');

    expect(themePackageSchema.parse(neoBrutalist)).toStrictEqual(neoBrutalist);
    expect(neoBrutalist.recipes).not.toEqual(goldBand.recipes);
    expect(neoBrutalist.schemes.light.semantic.primary)
      .not.toBe(goldBand.schemes.light.semantic.primary);
    expect(neoBrutalist.schemes.dark.material.shadow)
      .not.toBe(goldBand.schemes.dark.material.shadow);
  });

  it('lands the three visual directions in their real source packages', () => {
    const goldBand = getThemePackage('builtin.gold-band');
    const glass = getThemePackage('builtin.glass');
    const neoBrutalist = getThemePackage('builtin.neo-brutalist');

    expect(goldBand.version).toBe('1.1.0');
    expect(goldBand.schemes.light.semantic).toMatchObject({
      background: '#ffffff', primary: '#0d0d0d', ring: '#10a37f', sidebar: '#fafafa',
    });
    expect(goldBand.schemes.light.material).toMatchObject({ model: 'solid', radius: '0.75rem' });
    expect(goldBand.recipes.composer.material).toBe('elevated');

    expect(glass.version).toBe('1.3.0');
    expect(glass.schemes.light.material).toMatchObject({ model: 'liquid', blur: 22, saturate: 100 });
    expect(glass.schemes.light.semantic.primary).toBe('rgba(18,18,20,.92)');
    expect(glass.schemes.dark.semantic.primary).toBe('rgba(245,245,245,.92)');
    expect(glass.schemes.light.material.backgroundImage).not.toMatch(/(?:13,91,185|13,110,253|58,132,246)/u);
    expect(glass.schemes.dark.material.backgroundImage).not.toMatch(/(?:79,142,247|112,168,255)/u);
    expect(glass.recipes.card.material).toBe('subtle');
    expect(glass.recipes.editor.material).toBe('flat');

    expect(neoBrutalist.version).toBe('1.1.1');
    expect(neoBrutalist.name['zh-CN']).toBe('新粗野主义');
    expect(neoBrutalist.schemes.light.semantic).toMatchObject({
      background: '#f4f2ec', primary: '#161616', accent: '#ff5c35', sidebar: '#ebe8df',
    });
    expect(neoBrutalist.schemes.light.material).toMatchObject({ model: 'solid', radius: '0.25rem' });
    expect(neoBrutalist.schemes.light.material.backgroundImage).toBe('none');
    expect(neoBrutalist.recipes.sidebar.material).toBe('flat');
  });

  it('reserves full material shadow for elevated component roles', () => {
    const subtleGlassTitlebar = generatedRoleRule('builtin.glass', 'titlebar');
    const elevatedGlassComposer = generatedRoleRule('builtin.glass', 'composer');
    const flatBrutalistSidebar = generatedRoleRule('builtin.neo-brutalist', 'sidebar');

    expect(subtleGlassTitlebar).toContain('box-shadow:var(--gb-material-edge-shadow)');
    expect(subtleGlassTitlebar).not.toContain('box-shadow:var(--gb-material-shadow)');
    expect(elevatedGlassComposer)
      .toContain('box-shadow:var(--gb-material-shadow),var(--gb-material-edge-shadow)');
    expect(flatBrutalistSidebar).not.toContain('box-shadow:');
  });

  it('keeps glass typography and frosted dialog material inside the package contract', () => {
    const glass = getThemePackage('builtin.glass');

    expect(glass.schemes.light.typography.uiSize).toBe(14);
    expect(glass.schemes.dark.typography.uiSize).toBe(14);
    expect(glass.recipes.dialog).toMatchObject({
      background: 'popover',
      material: 'elevated',
    });
    expect(glass.schemes.light.semantic.popover).toContain('rgba(');
    expect(glass.schemes.dark.semantic.popover).toContain('rgba(');
    expect(glass.schemes.light.material.blur).toBeGreaterThan(0);
    expect(glass.schemes.dark.material.blur).toBeGreaterThan(0);
    expect(glass.schemes.light.material.model).toBe('liquid');
    expect(glass.schemes.dark.material.model).toBe('liquid');
    expect(glass.schemes.light.material.backdropContrast).toBeGreaterThan(100);
    expect(glass.schemes.dark.material.specularHighlight).not.toBe('none');
  });

  it('provides opaque popover surfaces for every non-glass theme', () => {
    for (const theme of builtinThemes.filter(({ id }) => id !== 'builtin.glass')) {
      expect(theme.schemes.light.semantic.popover).toMatch(/^#[\da-f]{6}$/iu);
      expect(theme.schemes.dark.semantic.popover).toMatch(/^#[\da-f]{6}$/iu);
      expect(theme.schemes.light.material.model).toBe('solid');
      expect(theme.schemes.dark.material.model).toBe('solid');
    }
  });

  it('rejects a package when the quality capability and profile disagree', () => {
    const glass = structuredClone(getThemePackage('builtin.glass'));
    glass.capabilities = glass.capabilities.filter((capability) => capability !== 'visual-quality-profiles');
    expect(themePackageSchema.safeParse(glass).success).toBe(false);
  });

  it('rejects quality overrides outside the closed material whitelist', () => {
    const glass = structuredClone(getThemePackage('builtin.glass')) as Record<string, unknown>;
    const profiles = glass.visualQualityProfiles as Record<string, unknown>;
    profiles.performance = {
      ...(profiles.performance as Record<string, unknown>),
      uiSize: 12,
    };
    expect(themePackageSchema.safeParse(glass).success).toBe(false);
  });

  it('rejects arbitrary recipe properties and incomplete schemes', () => {
    const invalidRecipe = structuredClone(getThemePackage('builtin.gold-band')) as Record<string, unknown>;
    const recipes = invalidRecipe.recipes as Record<string, Record<string, unknown>>;
    recipes.card.selector = '.business-card';
    expect(themePackageSchema.safeParse(invalidRecipe).success).toBe(false);

    const missingDark = structuredClone(getThemePackage('builtin.gold-band')) as Record<string, unknown>;
    delete (missingDark.schemes as Record<string, unknown>).dark;
    expect(themePackageSchema.safeParse(missingDark).success).toBe(false);
  });

  it('covers the required application, conversation, and workspace semantic roles', () => {
    const requiredTokens = [
      'contentHeader',
      'conversationBackground',
      'messageAssistant',
      'composer',
      'activity',
      'toolCard',
      'permissionCard',
      'workspaceTab',
      'resourceHeader',
      'fileTree',
      'editor',
      'diffAdded',
      'diffRemoved',
      'diffModified',
    ];

    for (const theme of builtinThemes) {
      for (const scheme of ['light', 'dark'] as const) {
        for (const token of requiredTokens) {
          expect(theme.schemes[scheme].semantic, `${theme.id}/${scheme} is missing ${token}`)
            .toHaveProperty(token);
        }
      }
    }
  });

  it('defines canonical personalization source semantics in both frontend and backend contracts', () => {
    const webTypes = readFileSync(path.resolve(__dirname, '../src/types.ts'), 'utf8');
    const rustConfig = readFileSync(path.resolve(__dirname, '../../src/config/mod.rs'), 'utf8');

    expect(webTypes).toContain('interface PersonalizationPreference');
    expect(webTypes).toContain("source: 'theme'");
    expect(webTypes).toContain("source: 'custom'");
    expect(rustConfig).toContain('pub struct PersonalizationPreference');
    expect(rustConfig).toContain('pub personalization: Option<PersonalizationPreference>');
  });
});

describe('appearance resolver', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('keeps system resolution inside the selected theme package', () => {
    vi.stubGlobal('window', { matchMedia: () => ({ matches: true }) });
    const effective = resolveAppearance(preference({ themeId: 'builtin.tech-neutral', colorScheme: 'system' }));
    expect(resolveColorScheme('system')).toBe('dark');
    expect(effective.themeId).toBe('builtin.tech-neutral');
    expect(effective.scheme).toBe(getThemePackage('builtin.tech-neutral').schemes.dark);
  });

  it('falls back to the safe built-in package and prunes stale quality entries', () => {
    const normalized = normalizeAppearancePreference(preference({
      themeId: 'user.missing',
      visualQualityByTheme: {
        'builtin.glass': 'performance',
        'builtin.gold-band': 'performance',
        'user.removed': 'full',
      },
    }));
    expect(normalized.themeId).toBe('builtin.gold-band');
    expect(normalized.visualQualityByTheme).toEqual({ 'builtin.glass': 'performance' });
  });

  it('isolates visual quality by stable theme id and restores each theme default', () => {
    const glassPerformance = appearanceWithQuality(
      preference({ themeId: 'builtin.glass' }),
      'performance',
    );
    expect(glassPerformance.visualQualityByTheme).toEqual({ 'builtin.glass': 'performance' });
    expect(resolveAppearance(glassPerformance).visualQuality).toBe('performance');

    const goldBand = appearanceWithTheme(glassPerformance, 'builtin.gold-band');
    expect(resolveAppearance(goldBand).visualQuality).toBeUndefined();
    expect(goldBand.visualQualityByTheme).toEqual({ 'builtin.glass': 'performance' });

    const glassAgain = appearanceWithTheme(goldBand, 'builtin.glass');
    expect(resolveAppearance(glassAgain).visualQuality).toBe('performance');
  });

  it('uses the package default without eagerly persisting it', () => {
    const initial = preference({ themeId: 'builtin.glass', visualQualityByTheme: {} });
    expect(resolveAppearance(initial).visualQuality).toBe('full');
    expect(initial.visualQualityByTheme).toEqual({});
  });

  it('limits performance mode differences to material effects', () => {
    const full = resolveAppearance(preference({ themeId: 'builtin.glass' }));
    const performance = resolveAppearance(preference({
      themeId: 'builtin.glass',
      visualQualityByTheme: { 'builtin.glass': 'performance' },
    }));
    expect(performance.scheme.semantic).toEqual(full.scheme.semantic);
    expect(performance.scheme.typography).toEqual(full.scheme.typography);
    expect(performance.scheme.avatars).toEqual(full.scheme.avatars);
    expect(performance.recipes).toEqual(full.recipes);
    expect(performance.material.blur).toBeLessThan(full.material.blur);
    expect(performance.material.textureOpacity).toBeLessThan(full.material.textureOpacity);
    expect(performance.material.backdropContrast).toBeLessThan(full.material.backdropContrast);
    expect(performance.material.specularHighlight).toBe('none');
  });

  it('applies one atomic root projection for theme, scheme, quality, and tokens', () => {
    const properties = new Map<string, string>();
    const classes = new Set<string>();
    const documentElement = {
      dataset: {} as Record<string, string>,
      classList: {
        toggle: (name: string, enabled: boolean) => enabled ? classes.add(name) : classes.delete(name),
      },
      style: {
        colorScheme: '',
        setProperty: (name: string, value: string) => properties.set(name, value),
      },
    };
    vi.stubGlobal('document', { documentElement });
    vi.stubGlobal('window', {});

    const effective = applyAppearance(preference({
      themeId: 'builtin.glass',
      colorScheme: 'light',
      visualQualityByTheme: { 'builtin.glass': 'performance' },
    }));

    expect(documentElement.dataset).toEqual({
      theme: 'builtin.glass',
      colorScheme: 'light',
      visualQuality: 'performance',
      materialModel: 'liquid',
    });
    expect(classes.has('dark')).toBe(false);
    expect(documentElement.style.colorScheme).toBe('light');
    expect(properties.get('--background')).toBe(effective.scheme.semantic.background);
    expect(properties.get('--gb-material-blur')).toBe(`${effective.material.blur}px`);
    expect(properties.get('--gb-material-backdrop-contrast')).toBe(`${effective.material.backdropContrast}%`);
    expect(properties.get('--gb-material-specular-highlight')).toBe(effective.material.specularHighlight);
    expect(properties.get('--gb-theme-ui-font-size')).toBe(`${effective.scheme.typography.uiSize}px`);
  });
});
