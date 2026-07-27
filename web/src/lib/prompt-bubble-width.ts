export interface PromptBubbleInlineSizeSources {
  labelInlineSizes: number[];
  visibleLineInlineSizes: number[];
  expandedHiddenLineInlineSizes: number[];
}

export function resolvePromptBubbleInlineSize({
  labelInlineSizes,
  visibleLineInlineSizes,
  expandedHiddenLineInlineSizes,
}: PromptBubbleInlineSizeSources) {
  return Math.ceil(Math.max(
    0,
    ...labelInlineSizes,
    ...visibleLineInlineSizes,
    ...expandedHiddenLineInlineSizes,
  ));
}
