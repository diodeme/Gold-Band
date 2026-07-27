import { readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';
import { desktopThemeOptions } from '../src/theme';
import type { ConcreteDesktopTheme } from '../src/types';

const expectedThemes = {
  light: {
    selector: /:root\[data-theme='light'\]\s*\{([\s\S]*?)\n\}/,
    background: '#fafafb',
    surface: '#ffffff',
    workspace: '#f1f2f5',
    border: '#e1e3e9',
    primary: '#5b6ba8',
    primaryForeground: '#ffffff',
    selection: '#cdd2e3',
    selectionForeground: '#191c24',
    messageUser: '#f0f1f5',
    messageUserForeground: '#191c24',
    contentHeader: 'var(--sidebar)',
    foreground: '#191c24',
    muted: '#667085',
    success: '#16794b',
    danger: '#c93c48',
  },
  'light-gray': {
    selector: /:root\[data-theme='light-gray'\]\s*\{([\s\S]*?)\n\}/,
    background: '#ffffff',
    surface: '#ffffff',
    workspace: '#ffffff',
    border: '#e5e5e5',
    primary: '#2f2f2f',
    primaryForeground: '#ffffff',
    selection: '#d9d9d9',
    selectionForeground: '#202020',
    messageUser: '#f3f3f3',
    messageUserForeground: '#202020',
    contentHeader: 'var(--sidebar)',
    foreground: '#2b2b2b',
    muted: '#666666',
    success: '#2e7954',
    danger: '#c23b4a',
  },
  dark: {
    selector: /:root,\s*:root\[data-theme='dark'\]\s*\{([\s\S]*?)\n\}/,
    background: '#181818',
    surface: '#242424',
    workspace: '#181818',
    border: '#333333',
    primary: '#313131',
    primaryForeground: '#f5f5f5',
    selection: '#555555',
    selectionForeground: '#ffffff',
    messageUser: '#2d2d2d',
    messageUserForeground: '#f2f2f2',
    contentHeader: 'var(--sidebar)',
    foreground: '#e8e8e8',
    muted: '#9a9a9a',
    success: '#59b68b',
    danger: '#df6b6b',
  },
  black: {
    selector: /:root\[data-theme='black'\]\s*\{([\s\S]*?)\n\}/,
    background: '#111111',
    surface: '#1b1b1b',
    workspace: '#111111',
    border: '#2b2b2b',
    primary: '#2d2d2d',
    primaryForeground: '#f2f2f2',
    selection: '#4d4d4d',
    selectionForeground: '#ffffff',
    messageUser: '#252525',
    messageUserForeground: '#f2f2f2',
    contentHeader: 'var(--sidebar)',
    foreground: '#e8e8e8',
    muted: '#929292',
    success: '#59b68b',
    danger: '#df6b6b',
  },
} as const satisfies Record<ConcreteDesktopTheme, ThemeExpectation>;

describe('desktop theme palettes', () => {
  const styles = readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');

  it.each(Object.entries(expectedThemes))('%s keeps runtime CSS and settings preview aligned', (themeId, palette) => {
    const theme = themeId as ConcreteDesktopTheme;
    const themeBlock = styles.match(palette.selector)?.[1] ?? '';
    const themeOption = desktopThemeOptions.find((option) => option.id === theme);

    expect(themeBlock).toContain(`--background: ${palette.background}`);
    expect(themeBlock).toContain(`--card: ${palette.surface}`);
    expect(themeBlock).toContain(`--gold-workspace: ${palette.workspace}`);
    expect(themeBlock).toContain(`--border: ${palette.border}`);
    expect(themeBlock).toContain(`--primary: ${palette.primary}`);
    expect(themeBlock).toContain(`--primary-foreground: ${palette.primaryForeground}`);
    expect(themeBlock).toContain(`--text-selection: ${palette.selection}`);
    expect(themeBlock).toContain(`--text-selection-foreground: ${palette.selectionForeground}`);
    expect(themeBlock).toContain(`--message-user: ${palette.messageUser}`);
    expect(themeBlock).toContain(`--message-user-foreground: ${palette.messageUserForeground}`);
    expect(themeBlock).toContain(`--content-header: ${palette.contentHeader}`);
    expect(themeBlock).toContain(`--foreground: ${palette.foreground}`);
    expect(themeBlock).toContain(`--muted-foreground: ${palette.muted}`);
    expect(themeBlock).toContain(`--gold-success: ${palette.success}`);
    expect(themeBlock).toContain(`--gold-danger: ${palette.danger}`);
    expect(themeOption?.preview).toEqual({
      background: palette.background,
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

  it('keeps the approved porcelain palette unchanged and replaces the warm palette with neutral technology gray', () => {
    const lightBlock = styles.match(expectedThemes.light.selector)?.[1] ?? '';
    const grayBlock = styles.match(expectedThemes['light-gray'].selector)?.[1] ?? '';
    const darkBlock = styles.match(expectedThemes.dark.selector)?.[1] ?? '';
    const blackBlock = styles.match(expectedThemes.black.selector)?.[1] ?? '';

    expect(lightBlock).toContain('--background: #fafafb');
    expect(lightBlock).toContain('--primary: #5b6ba8');
    expect(grayBlock).toContain('--sidebar: #f3f3f3');
    expect(grayBlock).toContain('--sidebar-foreground: #171717');
    expect(grayBlock).toContain('--sidebar-accent: #e7e7e7');
    expect(grayBlock).toContain('--sidebar-accent-foreground: #171717');
    expect(grayBlock).toContain('--title: #171717');
    expect(grayBlock).not.toContain('#8a6a32');
    expect(grayBlock).not.toContain('#52677f');
    expect(darkBlock).not.toContain('#4d9fff');
    expect(blackBlock).not.toContain('#a1aacb');
  });

  it('keeps theme choices name-only and removes the retired warm-light contract', () => {
    const settingsSource = readFileSync(path.resolve(__dirname, '../src/pages/SettingsPage.tsx'), 'utf8');
    const i18nSource = readFileSync(path.resolve(__dirname, '../src/i18n.ts'), 'utf8');

    expect(desktopThemeOptions.every((option) => !('descriptionKey' in option))).toBe(true);
    expect(settingsSource).not.toContain('option.descriptionKey');
    expect(i18nSource).not.toMatch(/theme(?:DefaultLight|TechGray|WarmLight|GoldDark|Black)Description/);
    expect(styles).not.toContain("data-theme='light-warm'");
  });
});

interface ThemeExpectation {
  selector: RegExp;
  background: string;
  surface: string;
  workspace: string;
  border: string;
  primary: string;
  primaryForeground: string;
  selection: string;
  selectionForeground: string;
  messageUser: string;
  messageUserForeground: string;
  contentHeader: string;
  foreground: string;
  muted: string;
  success: string;
  danger: string;
}

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
