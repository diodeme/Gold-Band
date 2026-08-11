import { describe, expect, it } from 'vitest';

import { shouldCollapseShellSidebar } from '@/lib/responsive-shell';

describe('responsive shell', () => {
  it('collapses the navigation rail on compact viewports', () => {
    expect(shouldCollapseShellSidebar(390)).toBe(true);
    expect(shouldCollapseShellSidebar(767)).toBe(true);
    expect(shouldCollapseShellSidebar(768)).toBe(false);
    expect(shouldCollapseShellSidebar(1280)).toBe(false);
  });
});
