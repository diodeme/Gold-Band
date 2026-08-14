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
    });
    expect(classes.has('dark')).toBe(false);
    expect(documentElement.style.colorScheme).toBe('light');
    expect(properties.get('--background')).toBe(effective.scheme.semantic.background);
    expect(properties.get('--gb-material-blur')).toBe(`${effective.material.blur}px`);
    expect(properties.get('--gb-theme-ui-font-size')).toBe(`${effective.scheme.typography.uiSize}px`);
  });
});
