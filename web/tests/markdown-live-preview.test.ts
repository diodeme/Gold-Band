import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { markdownImageSources } from '@/components/workspace/files/markdown-image-preview';

const editorSource = readFileSync(
  new URL('../src/components/workspace/files/WorkspaceFileEditor.tsx', import.meta.url),
  'utf8',
);
const atomicAdapterSource = readFileSync(
  new URL('../src/components/workspace/files/markdown-live-preview.ts', import.meta.url),
  'utf8',
);

describe('Markdown live preview integration', () => {
  it('extracts distinct local Markdown image sources without treating alt text as a path', () => {
    expect(markdownImageSources('![A](one.png)\n![B](<folder/two image.png>)\n![Again](one.png)')).toEqual([
      'one.png',
      'folder/two image.png',
    ]);
  });

  it('copies the current CodeMirror document and switches modes without mounting a second editor', () => {
    expect(editorSource).toContain('state.doc.toString()');
    expect(editorSource).toContain("previewMode ? 'source' : 'live-preview'");
    expect(editorSource.match(/<CodeMirror/g)).toHaveLength(1);
  });

  it('uses Atomic public live-preview extensions but never its raw-src image extension', () => {
    expect(atomicAdapterSource).toContain('atomic.inlinePreview');
    expect(atomicAdapterSource).toContain('atomic.highlightMarkdown');
    expect(atomicAdapterSource).toContain('enableTables ? [atomic.tables');
    expect(atomicAdapterSource).not.toContain('imageBlocks(');
  });
});
