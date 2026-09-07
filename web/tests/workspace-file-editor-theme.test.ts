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
const compatibilityStyles = readFileSync(new URL('../src/webview-compatibility.css', import.meta.url), 'utf8');

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
      'var(--link)',
      'var(--gold-running)',
      'var(--gold-success)',
      'var(--gold-warning)',
      'var(--gold-danger)',
    ]) {
      expect(editorExtensionsSource).toContain(token);
    }
  });

  it('keeps semantic diff line backgrounds and softens the inline changed-text layer', () => {
    expect(editorExtensionsSource).toContain("'.cm-deletedChunk, .cm-deletedLine': { backgroundColor: 'var(--workspace-editor-deleted-line)' }");
    expect(editorExtensionsSource).toContain("'.cm-insertedLine': { backgroundColor: 'var(--workspace-editor-inserted-line)' }");
    expect(editorExtensionsSource).toContain("'.cm-deletedText': { backgroundColor: 'var(--workspace-editor-deleted-text)' }");
    expect(editorExtensionsSource).toContain("backgroundColor: 'var(--workspace-editor-inserted-text)'");
    expect(styles).toContain('--workspace-editor-deleted-line: color-mix(in srgb, var(--destructive) 10%, transparent)');
    expect(styles).toContain('--workspace-editor-inserted-text: color-mix(in srgb, var(--gold-success) 12%, transparent)');
    expect(compatibilityStyles).toContain('--workspace-editor-deleted-line: transparent');
    expect(compatibilityStyles).toContain('--workspace-editor-inserted-text: var(--muted)');
    expect(editorExtensionsSource).toContain("backgroundImage: 'none'");
    expect(editorExtensionsSource).not.toContain('var(--destructive) 25%');
    expect(editorExtensionsSource).not.toContain('var(--gold-success) 22%');
    expect(editorExtensionsSource).not.toContain('&.cm-merge-b .cm-activeLine');
  });

  it('uses the application text-selection token for CodeMirror selections', () => {
    expect(editorExtensionsSource).toContain("backgroundColor: 'var(--text-selection)'");
    expect(editorExtensionsSource).not.toContain("backgroundColor: 'color-mix(in srgb, var(--primary) 20%, transparent)'");
  });

  it('maps Markdown links and code surfaces to contrast-safe semantic tokens', () => {
    expect(styles).toContain('--atomic-editor-link: var(--link)');
    expect(styles).toContain('--atomic-editor-code-bg: color-mix(in srgb, var(--gold-surface-high) 72%, var(--background))');
    expect(styles).not.toContain('--atomic-editor-link: var(--primary)');
    expect(compatibilityStyles).toContain("[data-webview-theme-rendering='fallback-tokens'] .workspace-markdown-live-preview");
    expect(compatibilityStyles).toContain('--atomic-editor-code-bg: var(--gold-surface-high)');
  });
});
