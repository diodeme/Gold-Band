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
});
