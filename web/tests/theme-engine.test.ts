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

function themeWithVisualQualityProfile() {
  const theme = structuredClone(getThemePackage('builtin.gold-band'));
  theme.capabilities.push('visual-quality-profiles');
  theme.visualQualityProfiles = {
    default: 'full',
    supported: ['full', 'performance'],
    performance: {
      blur: 0,
      saturate: 100,
      shadow: 'none',
      textureOpacity: 0,
      motionDuration: '0ms',
    },
  };
  return theme;
}

describe('theme package contract', () => {
  it('validates every built-in package with paired light and dark schemes', () => {
    expect(builtinThemes).toHaveLength(2);
    for (const theme of builtinThemes) {
      expect(themePackageSchema.parse(theme)).toStrictEqual(theme);
      expect(theme.schemes.light).toBeDefined();
      expect(theme.schemes.dark).toBeDefined();
    }
    expect(themePackageSummaries.map(({ id }) => id)).toEqual([
      'builtin.gold-band',
      'builtin.tech-neutral',
    ]);
  });

  it('registers both supported packages through the shared contract', () => {
    const goldBand = getThemePackage('builtin.gold-band');
    const techNeutral = getThemePackage('builtin.tech-neutral');

    expect(themePackageSchema.parse(techNeutral)).toStrictEqual(techNeutral);
    expect(techNeutral.recipes).not.toEqual(goldBand.recipes);
    expect(techNeutral.schemes.light.semantic.primary)
      .not.toBe(goldBand.schemes.light.semantic.primary);
    expect(techNeutral.schemes.light.semantic.link)
      .not.toBe(goldBand.schemes.light.semantic.link);
    expect(techNeutral.schemes.dark.elevation.overlay)
      .not.toBe(goldBand.schemes.dark.elevation.overlay);
  });

  it('lands the two supported visual directions in their real source packages', () => {
    const goldBand = getThemePackage('builtin.gold-band');
    const techNeutral = getThemePackage('builtin.tech-neutral');

    expect(goldBand.version).toBe('2.0.0');
    expect(goldBand.schemes.light.semantic).toMatchObject({
      background: '#ffffff', primary: '#0d0d0d', ring: '#10a37f', sidebar: '#fafafa',
    });
    expect(goldBand.schemes.light.material).toMatchObject({ model: 'solid' });
    expect(goldBand.schemes.light.shape.radiusControl).toBe('0.75rem');
    expect(goldBand.recipes.composer.material).toBe('elevated');

    expect(techNeutral.version).toBe('2.0.0');
    expect(techNeutral.schemes.light.semantic).toMatchObject({
      background: '#ffffff', primary: '#2f2f2f', sidebar: '#f3f3f3',
    });
    expect(techNeutral.schemes.light.material).toMatchObject({ model: 'solid' });
    expect(techNeutral.recipes.composer.material).toBe('flat');
  });

  it('projects each theme elevation token through the shared composer role', () => {
    const elevatedComposer = generatedRoleRule('builtin.gold-band', 'composer');
    const flatNeutralComposer = generatedRoleRule('builtin.tech-neutral', 'composer');

    expect(elevatedComposer).toContain('box-shadow:var(--gb-elevation-overlay)');
    expect(flatNeutralComposer).toContain('box-shadow:var(--gb-elevation-overlay)');
  });

  it('lets each package design message disclosures and runtime control surfaces', () => {
    const goldBand = getThemePackage('builtin.gold-band');
    const techNeutral = getThemePackage('builtin.tech-neutral');

    expect(goldBand.recipes['message-disclosure']).toMatchObject({
      background: 'activity', borderWidth: 'none', radius: 'control',
    });
    expect(techNeutral.recipes['message-disclosure']).toMatchObject({
      background: 'transparent', borderWidth: 'hairline', radius: 'control',
    });
    expect(goldBand.recipes['runtime-control']).toMatchObject({
      background: 'activity', borderWidth: 'none', radius: 'surface',
    });
    expect(techNeutral.recipes['runtime-control']).toMatchObject({
      background: 'tool-card', borderWidth: 'hairline', radius: 'control',
    });
    for (const theme of [goldBand, techNeutral]) {
      expect(theme.recipes.activity).toMatchObject({
        background: 'transparent', borderWidth: 'none', radius: 'none', elevation: 'none',
      });
      expect(theme.recipes['permission-card']).toMatchObject({
        background: 'transparent', border: 'border', borderWidth: 'hairline', elevation: 'none',
      });
    }

    expect(generatedRoleRule('builtin.gold-band', 'message-disclosure'))
      .toContain('background-color:var(--gb-recipe-background)');
    expect(generatedRoleRule('builtin.tech-neutral', 'runtime-control'))
      .toContain('border-color:var(--gb-recipe-border)');
  });

  it('provides opaque solid surfaces for every supported theme', () => {
    for (const theme of builtinThemes) {
      expect(theme.schemes.light.semantic.popover).toMatch(/^#[\da-f]{6}$/iu);
      expect(theme.schemes.dark.semantic.popover).toMatch(/^#[\da-f]{6}$/iu);
      expect(theme.schemes.light.material.model).toBe('solid');
      expect(theme.schemes.dark.material.model).toBe('solid');
    }
  });

  it('rejects a package when the quality capability and profile disagree', () => {
    const theme = themeWithVisualQualityProfile();
    theme.capabilities = theme.capabilities.filter((capability) => capability !== 'visual-quality-profiles');
    expect(themePackageSchema.safeParse(theme).success).toBe(false);
  });

  it('rejects quality overrides outside the closed material whitelist', () => {
    const theme = themeWithVisualQualityProfile() as unknown as Record<string, unknown>;
    const profiles = theme.visualQualityProfiles as Record<string, unknown>;
    profiles.performance = {
      ...(profiles.performance as Record<string, unknown>),
      uiSize: 12,
    };
    expect(themePackageSchema.safeParse(theme).success).toBe(false);
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
      'link',
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

  it('falls back from retired packages and prunes stale quality entries', () => {
    const normalized = normalizeAppearancePreference(preference({
      themeId: 'builtin.glass',
      visualQualityByTheme: {
        'builtin.glass': 'performance',
        'builtin.neo-brutalist': 'full',
        'builtin.gold-band': 'performance',
        'user.removed': 'full',
      },
    }));
    expect(normalized.themeId).toBe('builtin.gold-band');
    expect(normalized.visualQualityByTheme).toEqual({});
  });

  it('uses a safe light and locale fallback without browser globals', () => {
    vi.stubGlobal('window', undefined);
    vi.stubGlobal('document', undefined);
    vi.stubGlobal('navigator', undefined);
    expect(resolveColorScheme('system')).toBe('light');
    const effective = resolveAppearance(preference({ colorScheme: 'system' }));
    expect(effective.colorScheme).toBe('light');
    expect(effective.typography.ui.families[0]).toBe('Inter Variable');
  });

  it('resolves locale and script-specific UI fonts without leaking into the editor domain', () => {
    const simplifiedChinese = resolveAppearance(preference(), 'zh-CN');
    const english = resolveAppearance(preference(), 'en-US');
    const malformed = resolveAppearance(preference(), 'not_a_locale');

    expect(simplifiedChinese.typography.ui.families).toContain('Gold Band MiSans');
    expect(english.typography.ui.families).not.toContain('Gold Band MiSans');
    expect(simplifiedChinese.typography.editor.families).not.toContain('Gold Band MiSans');
    expect(simplifiedChinese.typography.ui.displayName).toBe('Inter Variable · MiSans');
    expect(malformed.typography.ui.families).toEqual(english.typography.ui.families);
  });

  it('normalizes malformed preferences to the canonical default', () => {
    const malformed = normalizeAppearancePreference({
      schemaVersion: 1,
      themeId: '',
      colorScheme: 'sepia',
      visualQualityByTheme: { 'builtin.gold-band': 'unbounded' },
    } as unknown as AppearancePreference);
    expect(malformed).toEqual(defaultAppearancePreference);
  });

  it('switches only between supported stable theme ids', () => {
    const techNeutral = appearanceWithTheme(preference(), 'builtin.tech-neutral');
    expect(techNeutral.themeId).toBe('builtin.tech-neutral');
    expect(resolveAppearance(techNeutral).themeId).toBe('builtin.tech-neutral');

    const retired = appearanceWithTheme(techNeutral, 'builtin.neo-brutalist');
    expect(retired.themeId).toBe('builtin.gold-band');
  });

  it('does not persist a visual quality choice for packages without that capability', () => {
    const initial = preference({ themeId: 'builtin.tech-neutral' });
    expect(appearanceWithQuality(initial, 'performance')).toBe(initial);
    expect(resolveAppearance(initial).visualQuality).toBeUndefined();
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
      themeId: 'builtin.tech-neutral',
      colorScheme: 'light',
      visualQualityByTheme: {},
    }));

    expect(documentElement.dataset).toEqual({
      theme: 'builtin.tech-neutral',
      colorScheme: 'light',
      visualQuality: 'full',
      materialModel: 'solid',
    });
    expect(classes.has('dark')).toBe(false);
    expect(documentElement.style.colorScheme).toBe('light');
    expect(properties.get('--background')).toBe(effective.scheme.semantic.background);
    expect(properties.get('--link')).toBe(effective.scheme.semantic.link);
    expect(properties.get('--gb-material-blur')).toBe(`${effective.material.blur}px`);
    expect(properties.get('--gb-material-backdrop-contrast')).toBe(`${effective.material.backdropContrast}%`);
    expect(properties.get('--gb-material-specular-highlight')).toBe(effective.material.specularHighlight);
    expect(properties.get('--gb-theme-ui-font-size')).toBe(`${effective.typography.ui.size}px`);
    expect(properties.get('--gb-radius-control')).toBe(effective.shape.radiusControl);
    expect(properties.get('--gb-elevation-overlay')).toBe(effective.elevation.overlay);
  });
});
