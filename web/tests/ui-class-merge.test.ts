import { describe, expect, it } from 'vitest';
import { cn } from '../src/lib/utils';

const uiFontSizeClasses = [
  'text-ui-nano',
  'text-ui-micro',
  'text-ui-caption',
  'text-ui-compact',
] as const;

describe('UI typography class merging', () => {
  it.each(uiFontSizeClasses)('keeps %s independent from text color utilities', (fontSizeClass) => {
    expect(cn('text-sm', fontSizeClass, 'text-foreground/80')).toBe(
      `${fontSizeClass} text-foreground/80`,
    );
  });

  it('lets the last font-size utility win without dropping text color', () => {
    expect(cn('text-ui-micro text-muted-foreground', 'text-xs')).toBe(
      'text-muted-foreground text-xs',
    );
    expect(cn('text-xs text-muted-foreground', 'text-ui-caption')).toBe(
      'text-muted-foreground text-ui-caption',
    );
  });

  it('keeps state-scoped colors separate from the base UI font size', () => {
    expect(
      cn(
        'text-ui-compact text-muted-foreground',
        'hover:text-foreground dark:text-muted-foreground/80',
      ),
    ).toBe(
      'text-ui-compact text-muted-foreground hover:text-foreground dark:text-muted-foreground/80',
    );
  });
});
