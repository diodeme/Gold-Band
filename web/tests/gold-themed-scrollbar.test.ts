import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { ACP_RAW_SCROLL_AREA_CLASS_NAME, ACP_SESSION_SCROLL_AREA_CLASS_NAME } from '../src/components/acp/ACPChatDialog';
import {
  GOLD_CONVERSATION_SCROLLBAR_CLASS,
  GOLD_THEMED_SCROLLBAR_CLASS,
  goldThemedScrollbarClassName,
} from '../src/lib/themed-scrollbar';
import { builtinThemes } from '../src/themes/builtin-themes';

function parseRgba(value: string) {
  const match = /^rgba\((\d+),(\d+),(\d+),(\d*\.?\d+)\)$/u.exec(value);
  expect(match, `${value} should be an explicit bounded rgba color`).not.toBeNull();
  return {
    rgb: match!.slice(1, 4).map(Number),
    alpha: Number(match![4]),
  };
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
    expect(styles).toContain('@supports not (scrollbar-width: thin)');
    expect(styles).not.toContain('scrollbar-width: auto');
    expect(styles).not.toContain('scrollbar-color: auto');
    expect(styles).not.toContain('@supports selector(::-webkit-scrollbar)');
  });

  it('keeps every theme scrollbar neutral and low contrast until hover', () => {
    for (const theme of builtinThemes) {
      for (const schemeName of ['light', 'dark'] as const) {
        const semantic = theme.schemes[schemeName].semantic;
        const track = parseRgba(semantic.scrollbarTrack);
        const thumb = parseRgba(semantic.scrollbarThumb);
        const hover = parseRgba(semantic.scrollbarThumbHover);

        expect(new Set(track.rgb).size, `${theme.id}/${schemeName} track should stay neutral`).toBe(1);
        expect(new Set(thumb.rgb).size, `${theme.id}/${schemeName} thumb should stay neutral`).toBe(1);
        expect(new Set(hover.rgb).size, `${theme.id}/${schemeName} hover should stay neutral`).toBe(1);
        expect(track.alpha).toBeLessThan(thumb.alpha);
        expect(thumb.alpha).toBeLessThan(hover.alpha);
        expect(hover.alpha).toBeLessThanOrEqual(0.4);
      }
    }
  });

  it('uses Gold Band tokens for shadcn ScrollArea scrollbars', () => {
    const scrollArea = readFileSync(path.resolve(__dirname, '../src/components/ui/scroll-area.tsx'), 'utf8');

    expect(scrollArea).toContain('p-[var(--gb-scrollbar-thumb-inset)]');
    expect(scrollArea).toContain('w-[var(--gb-scrollbar-width)]');
    expect(scrollArea).toContain('h-[var(--gb-scrollbar-width)]');
    expect(scrollArea).toContain('min-h-[var(--gb-scrollbar-min-length)]');
    expect(scrollArea).toContain('rounded-[var(--gb-scrollbar-thumb-radius)]');
    expect(scrollArea).toContain('bg-[var(--gold-scrollbar-track)]');
    expect(scrollArea).toContain('bg-[var(--gold-scrollbar-thumb)]');
    expect(scrollArea).toContain('bg-[var(--gold-scrollbar-thumb-hover)]');
    expect(scrollArea).not.toContain('bg-border');
  });

  it('keeps the conversation viewport scrollbar quieter than the global scrollbar', () => {
    const viewport = readFileSync(path.resolve(__dirname, '../src/components/conversation/ConversationViewport.tsx'), 'utf8');
    const styles = readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');

    expect(viewport).toContain('cn(GOLD_CONVERSATION_SCROLLBAR_CLASS, scrollClassName)');
    expect(GOLD_CONVERSATION_SCROLLBAR_CLASS).toBe('gold-conversation-scrollbar');
    expect(styles).not.toContain('--gold-conversation-scrollbar-track');
    expect(styles).toContain('--gold-conversation-scrollbar-thumb: color-mix(in srgb, var(--foreground) 11%, transparent)');
    expect(styles).toContain('--gold-conversation-scrollbar-thumb-hover: color-mix(in srgb, var(--foreground) 22%, transparent)');
    expect(styles).toMatch(/\.gold-themed-scrollbar\.gold-conversation-scrollbar:hover::-webkit-scrollbar-track \{\s*background: transparent;/u);
    expect(styles).toContain('.gold-themed-scrollbar.gold-conversation-scrollbar::-webkit-scrollbar-thumb');
    expect(styles).toMatch(/\.gold-themed-scrollbar\.gold-conversation-scrollbar \{\s*scrollbar-color: var\(--gold-conversation-scrollbar-thumb\) transparent;/u);
  });

  it('keeps the conversation scrollbar full-height while the composer uses a stable viewport layer', () => {
    const dialog = readFileSync(path.resolve(__dirname, '../src/components/acp/ACPChatDialog.tsx'), 'utf8');
    const viewport = readFileSync(path.resolve(__dirname, '../src/components/conversation/ConversationViewport.tsx'), 'utf8');

    expect(dialog).toContain('<ConversationViewportFooter');
    expect(dialog).toContain('data-acp-conversation-footer="viewport"');
    expect(dialog).not.toContain('"sticky bottom-0 z-20 mt-auto shrink-0",');
    expect(dialog).toContain('wallpaperSurface ? "bg-transparent" : "bg-background"');
    expect(dialog).toContain('className="absolute right-4 top-0 z-30 -translate-y-[calc(100%+1rem)]');
    expect(viewport).toContain('data-conversation-viewport-footer="true"');
    expect(viewport).toContain('paddingBottom: hasFooter');
    expect(viewport).toContain("new ResizeObserver");
  });

  it('keeps the right workspace Tab on the same themed native scrollbar path', () => {
    const styles = readFileSync(path.resolve(__dirname, '../src/styles.css'), 'utf8');
    const dock = readFileSync(path.resolve(__dirname, '../src/components/workspace/RightWorkspaceDock.tsx'), 'utf8');

    expect(dock).toContain('className="gold-themed-scrollbar flex min-w-0 flex-1');
    expect(dock).not.toContain('right-workspace-tab-scrollbar');
    expect(styles).not.toContain('.gold-themed-scrollbar.right-workspace-tab-scrollbar');
  });
});
