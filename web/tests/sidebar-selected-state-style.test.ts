import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

const selectedStateClass = 'bg-sidebar-accent text-sidebar-accent-foreground';
const invalidSelectedStateClass = 'bg-sidebar-accent text-sidebar-primary';

describe('sidebar selected-state styles', () => {
  it.each([
    '../src/components/conversation/ConversationSidebar.tsx',
    '../src/components/conversation/ConversationSessionSwitcher.tsx',
    '../src/components/Shell.tsx',
  ])('pairs the sidebar accent surface with its readable foreground in %s', (sourcePath) => {
    const source = readFileSync(path.resolve(__dirname, sourcePath), 'utf8');

    expect(source).toContain(selectedStateClass);
    expect(source).not.toContain(invalidSelectedStateClass);
  });

  it('uses sidebar foreground for navigation and section titles while preserving muted metadata', () => {
    const source = readFileSync(
      path.resolve(__dirname, '../src/components/conversation/ConversationSidebar.tsx'),
      'utf8',
    );

    expect(source).toContain('text-[14px] text-sidebar-foreground hover:bg-sidebar-accent');
    expect(source).toContain('tracking-[0.12em] text-sidebar-foreground');
    expect(source).toContain('tabular-nums text-muted-foreground');
  });

  it('uses simple semantic Lucide icons for context and run-mode navigation', () => {
    const source = readFileSync(
      path.resolve(__dirname, '../src/components/conversation/ConversationSidebar.tsx'),
      'utf8',
    );

    expect(source).toContain('icon={<Library />}');
    expect(source).toContain('icon={<Route />}');
    expect(source).not.toContain('icon={<Boxes />}');
    expect(source).not.toContain('icon={<Workflow />}');
  });
});
