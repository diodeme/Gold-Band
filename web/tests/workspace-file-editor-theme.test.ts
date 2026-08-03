import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const editorSource = readFileSync(
  new URL('../src/components/workspace/files/WorkspaceFileEditor.tsx', import.meta.url),
  'utf8',
);

describe('workspace file editor theme contract', () => {
  it('does not install the upstream light-only CodeMirror theme', () => {
    expect(editorSource).toContain('theme="none"');
    expect(editorSource).toContain("backgroundColor: 'transparent'");
  });

  it('uses application theme tokens for syntax highlighting', () => {
    expect(editorSource).toContain('syntaxHighlighting(workspaceHighlightStyle)');
    for (const token of [
      'var(--foreground)',
      'var(--muted-foreground)',
      'var(--primary)',
      'var(--gold-success)',
      'var(--gold-warning)',
      'var(--gold-danger)',
    ]) {
      expect(editorSource).toContain(token);
    }
  });
});
