import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

describe('responsive shell', () => {
  it('keeps manual sidebar intent separate from temporary viewport collapse', () => {
    const app = readFileSync(fileURLToPath(new URL('../src/App.tsx', import.meta.url)), 'utf8');
    const workspace = readFileSync(fileURLToPath(new URL('../src/components/workspace/WorkspaceShell.tsx', import.meta.url)), 'utf8');
    expect(app).not.toContain('shouldCollapseShellSidebar');
    expect(app).not.toContain('updateCompactShell');
    expect(app).toContain("localStorage.getItem('gold-band-sidebar-collapsed') === 'true'");
    expect(workspace).toContain('reduceWorkspaceAutoCollapse');
    expect(workspace).toContain('sidebarManuallyCollapsed: sidebarCollapsed');
  });
});
