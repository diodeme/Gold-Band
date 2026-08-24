import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { fileTreeIconStateClassName, fileTreeRowStateClassName } from '@/lib/file-tree-row-state';

const workspaceTreeSource = readFileSync(
  fileURLToPath(new URL('../src/components/workspace/files/WorkspaceFileTree.tsx', import.meta.url)),
  'utf8',
);
const conversationTreeSource = readFileSync(
  fileURLToPath(new URL('../src/components/workspace/ConversationDirectoryWorkspacePanel.tsx', import.meta.url)),
  'utf8',
);

describe('file tree row interaction states', () => {
  it('uses one theme token family for hover and selection while reserving a ring for focus', () => {
    const selected = fileTreeRowStateClassName(true, false).split(' ');
    const focused = fileTreeRowStateClassName(false, true).split(' ');
    const idle = fileTreeRowStateClassName(false, false).split(' ');

    expect(selected).toContain('bg-accent');
    expect(selected).toContain('text-accent-foreground');
    expect(selected).toContain('hover:bg-accent');
    expect(selected).not.toContain('ring-1');
    expect(focused).toContain('ring-1');
    expect(focused).toContain('ring-ring/60');
    expect(focused).not.toContain('bg-accent');
    expect(idle).toContain('hover:bg-accent/60');
    expect(idle).not.toContain('ring-1');
    expect(fileTreeIconStateClassName(true)).toBe('text-accent-foreground');
  });

  it('keeps both canonical-path file trees on the shared state projection', () => {
    for (const source of [workspaceTreeSource, conversationTreeSource]) {
      expect(source).toContain('fileTreeRowStateClassName');
      expect(source).toContain('fileTreeIconStateClassName');
      expect(source).not.toContain('bg-gold-running/12');
      expect(source).not.toContain('node.isFocused &&');
    }
  });
});
