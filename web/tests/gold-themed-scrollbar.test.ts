import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { ACP_RAW_SCROLL_AREA_CLASS_NAME, ACP_SESSION_SCROLL_AREA_CLASS_NAME } from '../src/components/acp/ACPChatDialog';
import { GOLD_THEMED_SCROLLBAR_CLASS, goldThemedScrollbarClassName } from '../src/lib/themed-scrollbar';

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
    expect(styles).toContain('--gold-scrollbar-track');
    expect(styles).toContain('--gold-scrollbar-thumb');
    expect(styles).toContain('--gold-scrollbar-thumb-hover');
    expect(styles).toContain(`.${GOLD_THEMED_SCROLLBAR_CLASS}::-webkit-scrollbar-thumb`);
    expect(styles).toContain(`.${GOLD_THEMED_SCROLLBAR_CLASS}::-webkit-scrollbar-button`);
  });
});
