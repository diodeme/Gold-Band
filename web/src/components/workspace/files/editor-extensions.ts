import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import type { Extension } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { tags } from '@lezer/highlight';

export const workspaceEditorTheme = EditorView.theme({
  '&': { height: '100%', backgroundColor: 'transparent', color: 'var(--foreground)' },
  '.cm-scroller': { fontFamily: 'var(--app-editor-font-family, ui-monospace)', fontSize: 'var(--app-editor-font-size, 12px)', lineHeight: '1.6' },
  '.cm-content': { padding: '12px 0' },
  '.cm-gutters': { backgroundColor: 'transparent', color: 'var(--muted-foreground)', borderRight: '1px solid color-mix(in srgb, var(--border) 55%, transparent)' },
  '.cm-activeLine, .cm-activeLineGutter': { backgroundColor: 'color-mix(in srgb, var(--muted) 35%, transparent)' },
  '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': { backgroundColor: 'var(--text-selection)' },
  '.cm-deletedChunk, .cm-deletedLine': { backgroundColor: 'color-mix(in srgb, var(--destructive) 10%, transparent)' },
  '.cm-insertedLine': { backgroundColor: 'color-mix(in srgb, var(--gold-success) 10%, transparent)' },
  '.cm-deletedText': { backgroundColor: 'color-mix(in srgb, var(--destructive) 25%, transparent)' },
  '&.cm-merge-b .cm-changedText': {
    backgroundColor: 'color-mix(in srgb, var(--gold-success) 22%, transparent)',
    backgroundImage: 'none',
  },
  '.cm-deletedLineGutter': { color: 'var(--destructive)' },
  '.cm-changedLineGutter': { color: 'var(--gold-success)' },
  '.cm-collapsedLines': { margin: '3px 8px', border: '1px solid color-mix(in srgb, var(--border) 70%, transparent)', borderRadius: '8px', backgroundColor: 'color-mix(in srgb, var(--muted) 45%, transparent)', color: 'var(--muted-foreground)' },
  '&.cm-focused': { outline: 'none' },
});

export const workspaceHighlightStyle = HighlightStyle.define([
  { tag: [tags.comment, tags.lineComment, tags.blockComment, tags.docComment], color: 'var(--muted-foreground)', fontStyle: 'italic' },
  { tag: [tags.meta, tags.processingInstruction, tags.punctuation], color: 'var(--muted-foreground)' },
  { tag: [tags.keyword, tags.controlKeyword, tags.operatorKeyword, tags.modifier], color: 'var(--gold-running)' },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName), tags.labelName], color: 'var(--gold-running)' },
  { tag: [tags.string, tags.special(tags.string), tags.regexp, tags.escape], color: 'var(--gold-success)' },
  { tag: [tags.number, tags.bool, tags.null, tags.atom], color: 'var(--gold-warning)' },
  { tag: [tags.invalid, tags.deleted], color: 'var(--gold-danger)' },
  { tag: [tags.heading, tags.strong], color: 'var(--foreground)', fontWeight: '600' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: [tags.link, tags.url], color: 'var(--link)', textDecoration: 'underline' },
]);

export const workspaceSyntaxHighlighting = syntaxHighlighting(workspaceHighlightStyle);

export async function loadWorkspaceLanguage(language: string | null): Promise<Extension | null> {
  if (!language) return null;
  const { languages } = await import('@codemirror/language-data');
  const normalized = language.toLowerCase();
  const description = languages.find((candidate) => (
    candidate.name.toLowerCase() === normalized
    || candidate.alias.some((alias) => alias.toLowerCase() === normalized)
  ));
  return description ? description.load() : null;
}

export async function loadWorkspaceLanguageForPath(path: string): Promise<Extension | null> {
  const { languages } = await import('@codemirror/language-data');
  const fileName = path.replaceAll('\\', '/').split('/').at(-1) ?? path;
  const extension = fileName.includes('.') ? fileName.split('.').at(-1)?.toLowerCase() : null;
  const description = languages.find((candidate) => (
    candidate.filename?.test(fileName)
    || (extension ? candidate.extensions.includes(extension) : false)
  ));
  return description ? description.load() : null;
}
