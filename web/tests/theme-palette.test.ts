import { readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';
import { getThemePackage, themePackageSummaries } from '../src/theme';

const expectedThemes = {
  light: {
    themeId: 'builtin.gold-band',
    colorScheme: 'light',
    background: '#fafafb', surface: '#ffffff', workspace: '#f1f2f5', border: '#e1e3e9',
    primary: '#5b6ba8', primaryForeground: '#ffffff', selection: '#cdd2e3',
    selectionForeground: '#191c24', messageUser: '#f0f1f5', messageUserForeground: '#191c24',
    foreground: '#191c24', muted: '#667085', success: '#16794b', danger: '#c93c48',
  },
  'light-gray': {
    themeId: 'builtin.tech-neutral',
    colorScheme: 'light',
    background: '#ffffff', surface: '#ffffff', workspace: '#ffffff', border: '#e5e5e5',
    primary: '#2f2f2f', primaryForeground: '#ffffff', selection: '#d9d9d9',
    selectionForeground: '#202020', messageUser: '#f3f3f3', messageUserForeground: '#202020',
    foreground: '#2b2b2b', muted: '#666666', success: '#2e7954', danger: '#c23b4a',
  },
  dark: {
    themeId: 'builtin.gold-band',
    colorScheme: 'dark',
    background: '#181818', surface: '#242424', workspace: '#181818', border: '#333333',
    primary: '#313131', primaryForeground: '#f5f5f5', selection: '#555555',
    selectionForeground: '#ffffff', messageUser: '#2d2d2d', messageUserForeground: '#f2f2f2',
    foreground: '#e8e8e8', muted: '#9a9a9a', success: '#59b68b', danger: '#df6b6b',
  },
  black: {
    themeId: 'builtin.tech-neutral',
    colorScheme: 'dark',
    background: '#111111', surface: '#1b1b1b', workspace: '#111111', border: '#2b2b2b',
    primary: '#2d2d2d', primaryForeground: '#f2f2f2', selection: '#4d4d4d',
    selectionForeground: '#ffffff', messageUser: '#252525', messageUserForeground: '#f2f2f2',
    foreground: '#e8e8e8', muted: '#929292', success: '#59b68b', danger: '#df6b6b',
  },
} as const;

describe('desktop theme palettes', () => {
  it.each(Object.entries(expectedThemes))('%s keeps runtime tokens and settings preview aligned', (_legacyName, palette) => {
    const theme = getThemePackage(palette.themeId);
    const scheme = theme.schemes[palette.colorScheme];
    const summary = themePackageSummaries.find(({ id }) => id === palette.themeId);

    expect(scheme.semantic).toMatchObject({
      background: palette.background,
      card: palette.surface,
      workspace: palette.workspace,
      border: palette.border,
      primary: palette.primary,
      primaryForeground: palette.primaryForeground,
      selection: palette.selection,
      selectionForeground: palette.selectionForeground,
      messageUser: palette.messageUser,
      messageUserForeground: palette.messageUserForeground,
      foreground: palette.foreground,
      mutedForeground: palette.muted,
      success: palette.success,
      danger: palette.danger,
    });
    expect(summary?.preview[palette.colorScheme]).toEqual({
      background: palette.workspace,
      surface: palette.surface,
      border: palette.border,
      primary: palette.primary,
      foreground: palette.foreground,
      muted: palette.muted,
      success: palette.success,
      danger: palette.danger,
    });
  });

  it.each(Object.entries(expectedThemes))('%s keeps body, muted, primary and semantic statuses at WCAG AA contrast', (_themeId, palette) => {
    expect(contrastRatio(palette.foreground, palette.background)).toBeGreaterThanOrEqual(7);
    expect(contrastRatio(palette.muted, palette.background)).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(palette.primaryForeground, palette.primary)).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(palette.selectionForeground, palette.selection)).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(palette.messageUserForeground, palette.messageUser)).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(palette.success, palette.background)).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(palette.danger, palette.background)).toBeGreaterThanOrEqual(4.5);
  });

  it('keeps the approved porcelain palette and technology-neutral package unchanged', () => {
    const light = getThemePackage('builtin.gold-band').schemes.light.semantic;
    const neutral = getThemePackage('builtin.tech-neutral').schemes.light.semantic;
    const dark = getThemePackage('builtin.gold-band').schemes.dark.semantic;
    const black = getThemePackage('builtin.tech-neutral').schemes.dark.semantic;

    expect(light.background).toBe('#fafafb');
    expect(light.primary).toBe('#5b6ba8');
    expect(neutral.sidebar).toBe('#f3f3f3');
    expect(neutral.sidebarForeground).toBe('#171717');
    expect(neutral.sidebarAccent).toBe('#e7e7e7');
    expect(neutral.sidebarAccentForeground).toBe('#171717');
    expect(neutral.title).toBe('#171717');
    expect(JSON.stringify(neutral)).not.toContain('#8a6a32');
    expect(JSON.stringify(neutral)).not.toContain('#52677f');
    expect(JSON.stringify(dark)).not.toContain('#4d9fff');
    expect(JSON.stringify(black)).not.toContain('#a1aacb');
  });

  it('keeps theme choices name-only and removes the retired warm-light contract', () => {
    const settingsSource = readFileSync(path.resolve(__dirname, '../src/pages/SettingsPage.tsx'), 'utf8');
    const i18nSource = readFileSync(path.resolve(__dirname, '../src/i18n.ts'), 'utf8');
    const stylesSource = readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');

    expect(themePackageSummaries.every((summary) => !('description' in summary))).toBe(true);
    expect(settingsSource).not.toContain('summary.description');
    expect(i18nSource).not.toMatch(/theme(?:DefaultLight|TechGray|WarmLight|GoldDark|Black)Description/);
    expect(stylesSource).not.toContain("data-theme='light-warm'");
  });
});

function contrastRatio(foreground: string, background: string) {
  const lighter = Math.max(relativeLuminance(foreground), relativeLuminance(background));
  const darker = Math.min(relativeLuminance(foreground), relativeLuminance(background));
  return (lighter + 0.05) / (darker + 0.05);
}

function relativeLuminance(hex: string) {
  const channels = hex.slice(1).match(/../g)?.map((channel) => Number.parseInt(channel, 16) / 255) ?? [];
  const [red = 0, green = 0, blue = 0] = channels.map((channel) =>
    channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4,
  );
  return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
}
