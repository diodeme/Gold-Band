import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { ACP_RAW_SCROLL_AREA_CLASS_NAME, ACP_SESSION_SCROLL_AREA_CLASS_NAME } from '../src/components/acp/ACPChatDialog';
import { GOLD_THEMED_SCROLLBAR_CLASS, goldThemedScrollbarClassName } from '../src/lib/themed-scrollbar';

const THEME_SCROLLBAR_TOKENS = {
  dark: { track: 4, thumb: 18, hover: 30 },
  black: { track: 4, thumb: 20, hover: 32 },
  light: { track: 3, thumb: 16, hover: 26 },
  'light-gray': { track: 3, thumb: 16, hover: 26 },
} as const;

function getThemeBlock(styles: string, theme: keyof typeof THEME_SCROLLBAR_TOKENS) {
  const normalizedStyles = styles.replace(/\r\n?/g, '\n');
  const selector = theme === 'dark'
    ? ":root,\n:root[data-theme='dark']"
    : `:root[data-theme='${theme}']`;
  const start = normalizedStyles.indexOf(`${selector} {`);
  const end = normalizedStyles.indexOf('\n}', start);

  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  return normalizedStyles.slice(start, end);
}

describe('Gold themed scrollbar', () => {
  it('keeps the themed scrollbar class attached to ACP scroll containers', () => {
    expect(ACP_SESSION_SCROLL_AREA_CLASS_NAME).toContain(GOLD_THEMED_SCROLLBAR_CLASS);
    expect(ACP_SESSION_SCROLL_AREA_CLASS_NAME).toContain('overflow-y-auto');
    expect(ACP_RAW_SCROLL_AREA_CLASS_NAME).toContain(GOLD_THEMED_SCROLLBAR_CLASS);
    expect(ACP_RAW_SCROLL_AREA_CLASS_NAME).toContain('overflow-y-auto');
  });

  it('keeps the utility composable with caller classes', () => {
    expect(goldThemedScrollbarClassName('h-full', false, 'overflow-auto')).toBe(
      `${GOLD_THEMED_SCROLLBAR_CLASS} h-full overflow-auto`,
    );
  });

  it('defines token-based scrollbar colors instead of relying only on color-scheme', () => {
    const styles = readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');

    expect(styles).toContain(`.${GOLD_THEMED_SCROLLBAR_CLASS}`);
    expect(styles).toContain('*::-webkit-scrollbar-thumb');
    expect(styles).toContain('*::-webkit-scrollbar-button');
    expect(styles).toContain('--gold-scrollbar-track');
    expect(styles).toContain('--gold-scrollbar-thumb');
    expect(styles).toContain('--gold-scrollbar-thumb-hover');
    expect(styles).toContain(`.${GOLD_THEMED_SCROLLBAR_CLASS}::-webkit-scrollbar-thumb`);
    expect(styles).toContain(`.${GOLD_THEMED_SCROLLBAR_CLASS}::-webkit-scrollbar-button`);
  });

  it('keeps every theme scrollbar neutral and low contrast until hover', () => {
    const styles = readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');

    for (const [theme, tokens] of Object.entries(THEME_SCROLLBAR_TOKENS)) {
      const themeBlock = getThemeBlock(styles, theme as keyof typeof THEME_SCROLLBAR_TOKENS);

      expect(themeBlock).toContain(
        `--gold-scrollbar-track: color-mix(in srgb, var(--foreground) ${tokens.track}%, transparent);`,
      );
      expect(themeBlock).toContain(
        `--gold-scrollbar-thumb: color-mix(in srgb, var(--foreground) ${tokens.thumb}%, transparent);`,
      );
      expect(themeBlock).toContain(
        `--gold-scrollbar-thumb-hover: color-mix(in srgb, var(--foreground) ${tokens.hover}%, transparent);`,
      );
      expect(tokens.track).toBeLessThan(tokens.thumb);
      expect(tokens.thumb).toBeLessThan(tokens.hover);
      expect(themeBlock).not.toMatch(/--gold-scrollbar-(?:track|thumb|thumb-hover):[^;]*var\(--primary\)/);
      expect(themeBlock).not.toMatch(/--gold-scrollbar-(?:track|thumb|thumb-hover):[^;]*var\(--muted-foreground\)/);
    }
  });

  it('uses Gold Band tokens for shadcn ScrollArea scrollbars', () => {
    const scrollArea = readFileSync(path.resolve(__dirname, '../src/components/ui/scroll-area.tsx'), 'utf8');

    expect(scrollArea).toContain('p-[3px]');
    expect(scrollArea).toContain('w-2.5');
    expect(scrollArea).toContain('h-2.5');
    expect(scrollArea).toContain('bg-[var(--gold-scrollbar-track)]');
    expect(scrollArea).toContain('bg-[var(--gold-scrollbar-thumb)]');
    expect(scrollArea).toContain('bg-[var(--gold-scrollbar-thumb-hover)]');
    expect(scrollArea).not.toContain('bg-border');
  });

  it('keeps the right workspace Tab scrollbar compact without native end buttons', () => {
    const styles = readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');

    expect(styles).toContain('.gold-themed-scrollbar.right-workspace-tab-scrollbar');
    expect(styles).toContain('height: 4px');
    expect(styles).toContain('.gold-themed-scrollbar.right-workspace-tab-scrollbar::-webkit-scrollbar-button');
  });
});
