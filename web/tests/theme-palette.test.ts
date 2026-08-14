import { readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';
import { getThemePackage, themePackageSummaries } from '../src/theme';

const expectedThemes = {
  light: {
    themeId: 'builtin.gold-band',
    colorScheme: 'light',
    background: '#ffffff', surface: '#ffffff', workspace: '#ffffff', border: '#e5e5e5',
    primary: '#0d0d0d', primaryForeground: '#ffffff', selection: '#d8efe8',
    selectionForeground: '#0d0d0d', messageUser: '#f5f5f5', messageUserForeground: '#0d0d0d',
    foreground: '#0d0d0d', muted: '#6e6e6e', success: '#0a7a5e', danger: '#c83237',
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
    background: '#0f0f0f', surface: '#171717', workspace: '#0f0f0f', border: '#2b2b2b',
    primary: '#f0f0f0', primaryForeground: '#0d0d0d', selection: '#155e4b',
    selectionForeground: '#ffffff', messageUser: '#242424', messageUserForeground: '#f0f0f0',
    foreground: '#f0f0f0', muted: '#9b9b9b', success: '#59b68b', danger: '#df6b6b',
  },
  black: {
    themeId: 'builtin.tech-neutral',
    colorScheme: 'dark',
    background: '#111111', surface: '#1b1b1b', workspace: '#111111', border: '#2b2b2b',
    primary: '#2d2d2d', primaryForeground: '#f2f2f2', selection: '#4d4d4d',
    selectionForeground: '#ffffff', messageUser: '#252525', messageUserForeground: '#f2f2f2',
    foreground: '#e8e8e8', muted: '#929292', success: '#59b68b', danger: '#df6b6b',
  },
  'editorial-light': {
    themeId: 'builtin.neo-brutalist',
    colorScheme: 'light',
    background: '#f4f2ec', surface: '#fffefa', workspace: '#e8e6df', border: '#161616',
    primary: '#161616', primaryForeground: '#ffffff', selection: '#ffd2c7',
    selectionForeground: '#161616', messageUser: '#eae7de', messageUserForeground: '#161616',
    foreground: '#161616', muted: '#5f5b52', success: '#157347', danger: '#c93838',
  },
  'editorial-dark': {
    themeId: 'builtin.neo-brutalist',
    colorScheme: 'dark',
    background: '#151515', surface: '#202020', workspace: '#111111', border: '#77736b',
    primary: '#f4f2ec', primaryForeground: '#161616', selection: '#ff6b4a',
    selectionForeground: '#161616', messageUser: '#262626', messageUserForeground: '#f4f2ec',
    foreground: '#f4f2ec', muted: '#b7b2a8', success: '#5cff8d', danger: '#ff5c5c',
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

  it('keeps the OpenAI-like default direction and technology-neutral package distinct', () => {
    const light = getThemePackage('builtin.gold-band').schemes.light.semantic;
    const neutral = getThemePackage('builtin.tech-neutral').schemes.light.semantic;
    const dark = getThemePackage('builtin.gold-band').schemes.dark.semantic;
    const black = getThemePackage('builtin.tech-neutral').schemes.dark.semantic;

    expect(light.background).toBe('#ffffff');
    expect(light.primary).toBe('#0d0d0d');
    expect(light.ring).toBe('#10a37f');
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
