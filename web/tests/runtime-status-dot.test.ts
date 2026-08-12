import { describe, expect, it } from 'vitest';

import { runtimeStatusDotClass } from '@/lib/runtime-status-dot';

describe('runtimeStatusDotClass', () => {
  it('uses the dedicated visible running token instead of the theme primary', () => {
    expect(runtimeStatusDotClass('running')).toContain('bg-gold-running');
    expect(runtimeStatusDotClass('running')).toContain('motion-safe:animate-pulse');
  });

  it('keeps stopped and unknown states visible', () => {
    expect(runtimeStatusDotClass('warning')).toBe('bg-yellow-500');
    expect(runtimeStatusDotClass('neutral')).toBe('bg-muted-foreground');
    expect(runtimeStatusDotClass(null)).toBe('bg-muted-foreground');
  });
});
