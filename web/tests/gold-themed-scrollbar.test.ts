import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { ACP_RAW_SCROLL_AREA_CLASS_NAME, ACP_SESSION_SCROLL_AREA_CLASS_NAME } from '../src/components/acp/ACPChatDialog';
import { GOLD_THEMED_SCROLLBAR_CLASS, goldThemedScrollbarClassName } from '../src/lib/themed-scrollbar';
import { builtinThemes } from '../src/themes/builtin-themes';

function parseRgba(value: string) {
  const match = /^rgba\((\d+),(\d+),(\d+),(\d*\.?\d+)\)$/u.exec(value);
  expect(match, `${value} should be an explicit bounded rgba color`).not.toBeNull();
  return {
    rgb: match!.slice(1, 4).map(Number),
    alpha: Number(match![4]),
  };
}

function parseHexRgb(value: string) {
  const match = /^#([\da-f]{2})([\da-f]{2})([\da-f]{2})$/iu.exec(value);
  expect(match, `${value} should be a six-digit foreground color`).not.toBeNull();
  return match!.slice(1, 4).map((channel) => Number.parseInt(channel, 16));
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
    for (const theme of builtinThemes) {
      for (const schemeName of ['light', 'dark'] as const) {
        const semantic = theme.schemes[schemeName].semantic;
        const foreground = parseHexRgb(semantic.foreground);
        const track = parseRgba(semantic.scrollbarTrack);
        const thumb = parseRgba(semantic.scrollbarThumb);
        const hover = parseRgba(semantic.scrollbarThumbHover);

        expect(track.rgb, `${theme.id}/${schemeName} track should stay neutral`).toEqual(foreground);
        expect(thumb.rgb, `${theme.id}/${schemeName} thumb should stay neutral`).toEqual(foreground);
        expect(hover.rgb, `${theme.id}/${schemeName} hover should stay neutral`).toEqual(foreground);
        expect(track.alpha).toBeLessThan(thumb.alpha);
        expect(thumb.alpha).toBeLessThan(hover.alpha);
        expect(hover.alpha).toBeLessThanOrEqual(0.4);
      }
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
