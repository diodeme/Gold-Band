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

  it('separates primary navigation, section labels, task titles, and metadata', () => {
    const source = readFileSync(
      path.resolve(__dirname, '../src/components/conversation/ConversationSidebar.tsx'),
      'utf8',
    );

    expect(source).toContain('text-sm text-sidebar-foreground hover:bg-sidebar-accent');
    expect(source).not.toContain('text-[14px] text-sidebar-foreground hover:bg-sidebar-accent');
    expect(source).toContain('text-xs font-medium text-muted-foreground');
    expect(source).toContain('text-sm font-bold leading-5 text-sidebar-foreground/80');
    expect(source).toContain('text-sidebar-foreground/85 cursor-pointer');
    expect(source).toContain('tabular-nums text-muted-foreground/55');
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
