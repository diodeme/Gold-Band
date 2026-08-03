import { describe, expect, it } from 'vitest';
import { inputVariants } from '@/components/ui/input';

describe('input visual variants', () => {
  it('keeps the standard form focus treatment as the default', () => {
    const classes = inputVariants();

    expect(classes).toContain('focus-visible:ring-[3px]');
    expect(classes).toContain('shadow-xs');
  });

  it('provides a restrained but visible focus treatment for toolbar filters', () => {
    const classes = inputVariants({ variant: 'toolbar' });

    expect(classes).toContain('focus-visible:ring-1');
    expect(classes).toContain('focus-visible:ring-ring/30');
    expect(classes).toContain('focus-visible:border-ring/55');
    expect(classes).not.toContain('focus-visible:ring-[3px]');
  });
});
