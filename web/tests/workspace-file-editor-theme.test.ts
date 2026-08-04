import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const editorSource = readFileSync(
  new URL('../src/components/workspace/files/WorkspaceFileEditor.tsx', import.meta.url),
  'utf8',
);
const editorExtensionsSource = readFileSync(
  new URL('../src/components/workspace/files/editor-extensions.ts', import.meta.url),
  'utf8',
);
const styles = readFileSync(new URL('../src/styles.css', import.meta.url), 'utf8');

describe('workspace file editor theme contract', () => {
  it('does not install the upstream light-only CodeMirror theme', () => {
    expect(editorSource).toContain('theme="none"');
    expect(editorExtensionsSource).toContain("backgroundColor: 'transparent'");
  });

  it('uses application theme tokens for syntax highlighting', () => {
    expect(editorExtensionsSource).toContain('syntaxHighlighting(workspaceHighlightStyle)');
    for (const token of [
      'var(--foreground)',
      'var(--muted-foreground)',
      'var(--gold-running)',
      'var(--gold-success)',
      'var(--gold-warning)',
      'var(--gold-danger)',
    ]) {
      expect(editorExtensionsSource).toContain(token);
    }
  });

  it('replaces the merge-view insertion underline with a solid highlight', () => {
    expect(editorExtensionsSource).toContain("'&.cm-merge-b .cm-changedText': {");
    expect(editorExtensionsSource).toContain("backgroundImage: 'none'");
    expect(editorExtensionsSource).not.toContain("'.cm-merge-b .cm-changedText'");
  });

  it('maps Markdown links and code surfaces to contrast-safe semantic tokens', () => {
    expect(styles).toContain('--atomic-editor-link: var(--gold-running)');
    expect(styles).toContain('--atomic-editor-code-bg: color-mix(in srgb, var(--gold-surface-high) 72%, var(--background))');
    expect(styles).not.toContain('--atomic-editor-link: var(--primary)');
  });
});
