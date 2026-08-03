import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { ensureSyntaxTree, syntaxTree } from '@codemirror/language';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { EditorState } from '@codemirror/state';
import { markdownImageSources } from '@/components/workspace/files/markdown-image-preview';
import { markdownHasTableImages } from '@/components/workspace/files/markdown-live-preview';

const editorSource = readFileSync(
  new URL('../src/components/workspace/files/WorkspaceFileEditor.tsx', import.meta.url),
  'utf8',
);
const atomicAdapterSource = readFileSync(
  new URL('../src/components/workspace/files/markdown-live-preview.ts', import.meta.url),
  'utf8',
);
const workspaceStyles = readFileSync(new URL('../src/styles.css', import.meta.url), 'utf8');

describe('Markdown live preview integration', () => {
  it('extracts distinct local Markdown image sources without treating alt text as a path', () => {
    expect(markdownImageSources('![A](one.png)\n![B](<folder/two image.png>)\n![Again](one.png)\n![Remote](https://example.com/a.png)\n<img src="html.png" alt="HTML" />')).toEqual([
      'one.png',
      'folder/two image.png',
      'html.png',
    ]);
  });

  it('keeps tables enabled unless a valid GFM table itself contains an image', () => {
    expect(markdownHasTableImages('| Name | Value |\n| --- | --- |\n| plain | text |\n\n![outside](safe.png)')).toBe(false);
    expect(markdownHasTableImages('| Name | Value |\n| --- | --- |\n| icon | ![inside](unsafe.png) |')).toBe(true);
    expect(markdownHasTableImages('![outside](not-a-table.png)\n| Name | Value |\n| data | text |\n| --- | --- |')).toBe(false);
  });

  it('parses valid GFM tables with the configured Markdown language base', () => {
    const state = EditorState.create({
      doc: '| 状态 | 说明 |\n| --- | --- |\n| 完成 | 表格已渲染 |',
      extensions: markdown({ base: markdownLanguage }),
    });
    const tree = ensureSyntaxTree(state, state.doc.length, 200) ?? syntaxTree(state);
    const nodeNames: string[] = [];
    tree.iterate({ enter: (node) => { nodeNames.push(node.name); } });
    expect(nodeNames).toContain('Table');
  });

  it('uses one CodeMirror view and rebuilds stable mode profiles with semantic viewport state', () => {
    expect(editorSource).toContain('state.doc.toString()');
    expect(editorSource).toContain('state.toJSON({ history: historyField })');
    expect(editorSource).toContain('captureEditorViewportAnchor');
    expect(editorSource).toContain('key={editorProfileKey}');
    expect(editorSource).toContain('appliedTargetRevisionsRef.current.get(documentKey)');
    expect(editorSource).toContain('viewportAnchorDocumentTop');
    expect(editorSource).toContain('targetMeasured');
    expect(editorSource).toContain('ResizeObserver');
    expect(editorSource).toContain('basicSetup={basicSetup}');
    expect(editorSource).toContain('onChange={handleChange}');
    expect(editorSource).not.toContain('sourceEditorRef');
    expect(editorSource.match(/<CodeMirror/gu)).toHaveLength(1);
  });

  it('constrains Atomic tables to the file detail width and wraps cell content', () => {
    expect(workspaceStyles).toMatch(/\.workspace-markdown-live-preview \.cm-atomic-table table \{[\s\S]*?table-layout: fixed;/u);
    expect(workspaceStyles).toMatch(/\.workspace-markdown-live-preview \.cm-atomic-table th,[\s\S]*?overflow-wrap: anywhere;/u);
  });

  it('uses Atomic public live-preview extensions but never its raw-src image extension', () => {
    expect(atomicAdapterSource).toContain('atomic.inlinePreview');
    expect(atomicAdapterSource).toContain('atomic.highlightMarkdown');
    expect(atomicAdapterSource).toContain('enableTables ? [atomic.tables');
    expect(atomicAdapterSource).not.toContain('imageBlocks(');
  });
});
