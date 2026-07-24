import { describe, expect, it } from 'vitest';

import { resolvePromptBubbleInlineSize } from '@/lib/prompt-bubble-width';

describe('prompt bubble width', () => {
  it('uses the widest visible source while hidden content is collapsed', () => {
    expect(resolvePromptBubbleInlineSize({
      labelInlineSizes: [248],
      visibleLineInlineSizes: [620, 884, 731],
      expandedHiddenLineInlineSizes: [],
    })).toBe(884);
  });

  it('includes expanded hidden content and rounds fractional layout widths', () => {
    expect(resolvePromptBubbleInlineSize({
      labelInlineSizes: [248.2],
      visibleLineInlineSizes: [884.1],
      expandedHiddenLineInlineSizes: [1032.4],
    })).toBe(1033);
  });
});
