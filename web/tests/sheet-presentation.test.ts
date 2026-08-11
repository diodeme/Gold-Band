import { describe, expect, it } from 'vitest';
import { resolveSheetOverlayVisibility } from '../src/components/ui/sheet';

describe('Sheet presentation contract', () => {
  it('does not render a page-dimming overlay for a non-modal side drawer', () => {
    expect(resolveSheetOverlayVisibility(false)).toBe(false);
  });

  it('keeps the overlay for modal sheets and allows an explicit override', () => {
    expect(resolveSheetOverlayVisibility(true)).toBe(true);
    expect(resolveSheetOverlayVisibility(true, false)).toBe(false);
    expect(resolveSheetOverlayVisibility(false, true)).toBe(true);
  });
});
